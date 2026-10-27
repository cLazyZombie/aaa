## E81 Reader 사용 설정 준비·E109 종료 증거(t-1218)

- [원본 macOS 웹 job](https://github.com/cLazyZombie/aaa/actions/runs/40584602186/job/122472617885)의
  전체 로그와 artifact 목록을 확보했다. E81 원문은 `false !== true`이며 stack·Reader 실패 화면은 없다.
  E109는 `getByTestId('terminal')`의 detached 대기 중 page/context/browser closed다. 원본 artifact에는
  `return-failure-390.json`·브라우저 stderr/종료 신호·page crash/close/disconnected 기록이 없다.
  원래 E109의 JSON은 실험실 루트에만 썼으며 공통 수집기는 diagnostics 폴더만 복사했다.
- E81의 같은 Reader owner에서 실제 account_pool 응답 전달만 gate로 보류한다. 서버 members에 cold가
  있는데 목록 checkbox는 checked=false/disabled=true인 경계에서 기존 단언이 실패했다. 목록 생성은
  설정 반영 완료가 아니다. 응답 도착을 확인하고 gate를 풀어 checked/disabled DOM 사건을 기다린 뒤
  기존 checked=true·disabled=true 단언을 그대로 수행한다. forbidden·응답/host 파일의 인증 비노출·
  Reader 메일 비노출도 유지한다. gate는 원래 fetch·응답을 쓰며 Service Worker 경유도 같은 경계를 지난다.
  새 대기의 상한은5초다. 실패 때 원래 stack·응답 종류/상태/비밀 없는 계정 별칭·DOM·그림을 저장한다.
- E109의 `returnDiagnostics`는 renderer에 다시 묻기 전에 최초 오류/stack을 기록한다. page crash/close,
  context close, browser disconnected, 명시적 정리 시작/끝/실패, Playwright 소유 launcher의 stderr와
  종료 코드/신호를 diagnostics에 둔다. launcher는 macOS pw_run.sh이므로 그 신호를 별도 WebProcess의
  신호로 단정하지 않는다. 조회·정리 오류가 최초 오류를 덮지 않으며 실패 JSON도 공통 artifact 수집에 들어간다.
  기존12회 왕복·같은 xterm/연결·휠 한 번·실제 입력·숨김/만료·Reader 비전송 단언과 시한은 바꾸지 않는다.
- `NODE_PATH=<격리 node_modules> nice -n 15 node --test web/e2e/return-diagnostics.test.mjs`의 두 keeper는
  저장소 고정 Playwright1.64.1과 공용 WebKit을 쓴다. 실제 DOM 분리의 성공 뒤 명시적 정리와 정상 exit를
  구별하고, 실제 selector가 기다리기 시작한 뒤 공개 browser.close를 호출해 최초 locator 오류·다음 조회 오류·
  정리 오류의 독립 보존을 확인한다. 필요성: 페이지 종료 뒤 진단도 실패하면 최초 오류와 산출물을 잃는다.
  구 수집 순서는 두 번째 keeper에서 page.evaluate 오류로 실패했고 수리 뒤 두 keeper는 각각1초 미만이다.
  이 제어된 종료는 원본 CI의 종료 주체/원인을 재현했다는 뜻이 아니다.
- E109 원본과 수명 계측판은 로컬 단독에서 모두 통과했다. 이 결과를 원본 CI 종료의 회복이나
  부하/환경 원인으로 판정하지 않는다. E109 종료 원인은 미확정이며 다음 실패의 위 증거가 필요하다.
  E81 수리와 E109 증거 수집을 구별해 보고하고 전체 수리 완료로 표시하지 않는다.
- 참고 후보는 [React ChangeEventPlugin](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/plugins/ChangeEventPlugin.js)
  ([MIT](https://github.com/facebook/react/blob/main/LICENSE))의 controlled checkbox commit과
  [Playwright page events](https://playwright.dev/docs/api/class-page#page-event-crash)·
  [browser disconnected](https://playwright.dev/docs/api/class-browser#browser-event-disconnected)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))의 수명 구분이다.
  외부 코드·의존성은 가져오지 않는다. 고정 sleep·재시도·시한 연장·기대값 완화·시험 삭제·새 E2E 번호는 없다.

## E2E16 복귀 크기 기준·E2E28 새 hover 진입(t-1209)

- [원본 macOS 웹 job](https://github.com/cLazyZombie/aaa/actions/runs/40521069216/job/122247985055)의
  전체 E2E 로그·실패 JSON/PNG·실행 시각 artifact를 확보했다. E16은 설정 중 112×38,
  복귀 요청 112×35와 응답, 창 높이 증가 뒤 112×36과 응답까지 왔다. 실패 뒤에도 같은 링크의
  up은 파일 창을 열었다. 원본은 `before` 변수와 실패 순간 제목/DOM 행 수를 직접 기록하지 않았다.
- 설정을 가린 터미널은 더 큰 칸에서 맞춰진다. 복귀 commit 뒤 ResizeObserver 전에는 옛
  `fit=same`·제목·DOM 행 수가 모두 38로 맞아도 실제 칸은 35행이다. `fittedTerminalGrid`는
  현재 부모 높이·xterm padding·실제 행 높이까지 대조해 이 기준값을 거절한다. 새 관측 keeper는
  옛 세 표지의 자체 일치, 늦은 ACK, 실제 행 반영과 pending을 구별한다. 필요한 이유는
  잘못 읽은 38행에 1을 더한 값을 기다리는 준비 경쟁을 막기 위해서다. 기존 112열·한 행 증가·
  좌표 불변·실제 PTY 재그림·선택/링크·가상화/줄 참조/설정 기록 계약은 유지한다.
- 원본 E28은 행 `hover=true`·카드 없음·요약 응답 200이었다. 진행 목록 검사의 reload가
  DOM 사건 수집기를 지워 당시 닫힘 원인이 resize인지 목록 scroll인지 소급 확정할 수 없다.
  수집기는 addInitScript로 reload 뒤에도 설치하며 테마 경계·좌표·관련 요소·scroll·응답·표시를 남긴다.
- 같은 E28 desktop light owner에 실제 테마 변경 → 카드 표시 → native resize 두 번으로 원래
  창 크기 복귀 → 포인터는 같은 행·카드 없음 순서를 넣었다. 기존 같은 좌표 `hover()`는
  새 mouseover 없이 원본과 같은 30초 오류로 실패했다. 각 테마 검사는 행 밖 이동 뒤 실제
  진입을 한 번 수행한다. 동일 재현이 통과하며 light/dark 카드·상세, Chromium320/WebKit390
  Reader·읽기 전용·실제 모델 관측/재시작 계약도 그대로 검사한다. resize 전달은 미리 구독한
  native 사건으로 끝낸다. 실패 재시도·시한 확대·기대값 완화·고정 sleep은 추가하지 않는다.
- E16 실패 관측은 실제 제목·행 수·칸/행 높이·fit과 기존 동기 WebSocket 전송/상태 기록을
  함께 남긴다. 스크린샷 폴더를 지정하면 성공 경로에도 누름 중 resize와 모델 카드 사건을 저장한다.
- 참고 후보는 [xterm 5.5.0 FitAddon](https://github.com/xtermjs/xterm.js/blob/5.5.0/addons/addon-fit/src/FitAddon.ts)
  (MIT)의 실제 칸 치수, [Playwright mouse 시험](https://github.com/microsoft/playwright/blob/main/tests/page/page-mouse.spec.ts)
  (Apache-2.0)의 실제 포인터 이동, [React EnterLeaveEventPlugin](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/plugins/EnterLeaveEventPlugin.js)
  (MIT)의 진입/이탈 경계다. 외부 코드·의존성·제품 동작은 바꾸지 않는다.
- 부분 검증은 웹 유닛 전체·타입/번들, 기존 E16/E28 전체와 E03 드래그/제자리 클릭,
  공개 검사다. Playwright 1.64.1과 공용 브라우저 캐시를 쓴다. 전체 ci-local·cross·병합·push는
  정규 착지 차례에서 수행한다.

## E103 재구독의 같은 기록 알림과 복귀 읽기(t-1216 착지 수리)

- 첫 착지의 E103은 지연 회선 Chromium1280/coordinator/trial1에서 최신 요청2회로 실패했다.
  보존된 return-body는2342.5ms·응답1회다. t-1147의 protocol은 count/lookup/commit의 DOM·Runtime만
  기록하므로 복귀 구간의 Network 요청·예약 호출 스택은 없다. 이 원문만으로 두 번째 예약자를
  소급 확정했다고 하지 않는다. t-1147 보고와 t-1134의 이전 read/복귀 경계도 먼저 읽었다.
- 같은 합성 fixture의 별도 관측에서 복귀 effect의 `refresh()`로 최신 요청이 시작된 뒤,
  재구독이 기존과 같은 signature의 chat_changed를 재전달하는 순서를 확보했다.
  api가 signature를 버려 `invalidate()`가2초 trailing read를 남기는 실제 경로가 있다.
  해당 순서를 실제 ConversationRefresh의 응답 보류 gate와 정지 단조 시계로 고정하면
  수리 전 처음1회+복귀1회가 아니라3회로 실패한다. 부하·간헐로 결론내지 않는다.
- API가 이미 받는 signature를 내부 ChatNote로 전달하고, 같은 pane의 마지막 signature를
  lane에 유지한다. 같은 값의 재전달은 추가 invalidate를 만들지 않는다. 다른 signature는
  읽는 중에도 다음 확인을 남기며 다른 pane으로 바뀌면 이전 signature를 잊는다.
  복귀의 강제 최신 확인·2초 변경 경계·10초 backstop·직렬화·숨김 정리와 wire는 유지한다.
- 새 `conversationRefresh.test.mjs` keeper1개가 필요한 이유는 재구독의 중복 확인을 막으면서
  실제 변경·다른 pane의 확인을 잃지 않아야 하기 때문이다. 응답 gate의 완료로 끝나고 실제로
  2초를 자지 않는다. 수리 전 실패 원문과 수리 후 통과를 보존한다. fixture 정리 순서와 다른
  pane read 시작의 비동기 경계 오류도 원문을 보존한 뒤 사건 대기로 수리했다.
- 기존 E103의 모든 조합·trial·과거 위치·동일 DOM·초안·과거 요청0회·복귀 최신1회·시한을
  그대로 확인한다. 시험 집계 창·기대값·시한·데이터·단계는 바꾸지 않는다.
- 참고는 [VS Code Event.latch](https://github.com/microsoft/vscode/blob/main/src/vs/base/common/event.ts)
  ([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt))와
  [RxJS distinctUntilChanged](https://github.com/ReactiveX/rxjs/blob/7.8.2/src/internal/operators/distinctUntilChanged.ts)
  ([Apache-2.0](https://github.com/ReactiveX/rxjs/blob/7.8.2/LICENSE.txt))다.
  같은 키의 연속 알림을 걸러 내는 원칙만 참고하며 외부 코드·실행 의존성을 추가하지 않는다.

## E117 터미널 인용의 실제 행 준비(t-1208 착지 수리)

- main `165797c0`·후보 `8d8491b1`의 변경 없는 E117을 각각 한 번 단독 실행했고 모두 통과했다.
  병렬 실패 원문은 `selectNodeContents(undefined)`이며, 대상 검색과 Range 설정은 한 동기 호출이다.
  그 사이 DOM이 교체됐다고 단정하지 않는다. 실제 host snapshot을 gate로 붙잡으면 래퍼·빈 xterm 행만
  있는 상태에서 같은 TypeError가 결정적으로 재현된다. agent 출력 완료와 브라우저 행 준비는 다르다.
- gate를 푼 첫 snapshot 그림 직후에는 autoFit resize가 다시 그려 좌표가 한 줄 밀리는 추가 실패도 관측했다.
  `Term`의 기존 onRender 관측 표지에 그린 cols/rows를 더한다. E117은 마지막 송신 resize와 그린 크기,
  fit 완료와 두 원문 행을 DOM 사건으로 확인한 뒤 좌표를 읽는다. 제품의 입력·인용 계약은 바꾸지 않는다.
- 기존 입력줄 owner에 실제 WebSocket 중계 gate를 둔다. 빈 래퍼를 준비로 세지 않음을 확인한 뒤 서버의
  원래 메시지를 같은 순서로 한 번만 풀고, 기존 실제 마우스 드래그·두 줄 원문·출처·연속 줄 번호·focus·
  PTY 미전송·오류 없음 단언을 모두 유지한다. 새 E2E 번호/단계·고정 sleep·재시도는 없다.
  행 준비의 실패 상한은5초이며 기존 시험의 시한·단언을 늘리거나 완화하지 않는다.
- 최초 병렬 실패에는 해당 페이지가 finally에서 닫히기 전의 행 진단이 없다. 남은 공통 페이지의 t1 터미널
  진단을 실패 페이지의 pane으로 해석하지 않는다. gate의 같은 오류와 제품 render 수명으로 원인을 판정한다.
- 참고 후보는 [xterm 5.5 DOM renderer](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/renderer/dom/DomRenderer.ts)
  ([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))와
  [Playwright WebSocketRoute](https://playwright.dev/docs/api/class-websocketroute)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))다.
  xterm의 행 자식 교체·비동기 렌더 수명과 실제 서버 중계 gate를 대조했으며 외부 코드·의존성을 가져오지 않았다.

## E42 맥락 단추의 재그리기와 키 focus(t-1208 착지 수리)

- main `165797c0`와 후보 `762c39fc`의 원래 E42 전체를 각각 한 번 단독 실행했고 모두 약13.1초에 통과했다.
  병렬 최초 실패의 원문에는 HTTP/키 사건 trace가 없어 그 순간의 순서를 소급 단정하지 않는다.
  별도 사건 gate에서 단추 focus와 Enter 사이에 실제 focus 재조회 응답을 놓아 같은 f60 대기를 재현했다.
  DOM 교체 → focus가 BODY → Enter가 BODY → full_context 요청0회다. agent 읽기·host 중계 전에 입력이 사라졌다.
  부하나 시한으로 닫지 않고 실제 재그리기 경계를 수리한다.
- `labelContextButtons`는 같은 파일·hunk·방향 단추가 같은 render에서 제거·생성된 경우에만 focus를 복구한다.
  연결된 옛 단추에서 다른 곳으로 이동한 focus·unmount·다른 창은 복구하지 않는다. 새 입력을 보내거나 원문을 선행 읽지 않는다.
  기존 한글 이름·Enter/Space·20줄/전체·한 줄/나란히·가상화·44px 계약은 유지한다.
- E42의 새 keeper `데스크톱 맥락: 재조회 중 단추 focus와 다른 조작의 focus를 지킨다`는 실제 HTTP 응답과
  다음 렌더 frame을 사건 gate로 사용한다. 다시 focus하는 locator.press 대신 native keyboard 입력을 보낸다.
  수리 전 focus 소실로 실패했고 수리 후 full_context1회·HTTP200/full_context=true·20줄/전체/끝 줄까지 확인한다.
  필요성: 기존 단독 성공만으로 가상 DOM 교체 사이의 다음 키 유실을 막을 수 없다. 개별 keeper는5초 미만이다.
  실패 진단에는 full_context 요청 수·query도 남긴다. 기존 단언·시한·시험을 완화/삭제하지 않는다.
- 휴대폰의 visible 확인과 boundingBox 사이에 단추가 교체돼 null이 된 원문도 보존했다. 크기는 같은 DOM 호출에서
  getBoundingClientRect로 읽고 기존44px 단언·실제 tap을 유지한다. 실패 재시도·고정 sleep·시한 확대는 없다.
- 참고는 고정 [Pierre FileDiff](https://github.com/pierrecomputer/pierre/blob/diffs-v1.5.2/packages/diffs/src/components/FileDiff.ts)와
  [CodeView React wrapper](https://github.com/pierrecomputer/pierre/blob/diffs-v1.5.2/packages/diffs/src/react/CodeView.tsx)(Apache-2.0),
  [VS Code diff view model](https://github.com/microsoft/vscode/blob/main/src/vs/editor/browser/widget/diffEditor/diffEditorViewModel.ts)
  ([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt))다. 실제 DOM 수명과 맥락 표시/입력 상태 분리를 대조했다.
  외부 코드·의존성을 추가하지 않는다.

## Windows idle-audit의 스택 사용과 실패 수집(t-1208)

- [원본 Windows job](https://github.com/cLazyZombie/aaa/actions/runs/40521069216/job/122247985312)
  (main `3ee2b329`)의 idle-audit owner가 `has overflowed its stack` 뒤
  `0xc00000fd, STATUS_STACK_OVERFLOW`로 끝났다. 같은 코드의 Windows 격리 실행에서도 재현했다.
  직접 만든 외부 디버거가 소유한 자식의 Windows debug event·DbgHelp stack walk를 읽었다.
  시험 poll → `Host::call` → `Host::call_from` → 요청 가림 직렬화 → 할당 순서이며 재귀는 관측되지 않았다.
  원본의 인접 stack pointer 차이는 시험 poll 757,008바이트·call 48,016바이트·call_from 1,115,312바이트다.
  예외의 마지막 할당 함수 하나를 원인으로 단정하지 않는다.
- 같은 Windows verify profile의 `-Zprint-type-sizes` 관측에서 시험 body future는
  27,440→6,384바이트다. call future 23,456바이트·call_from future 23,152바이트는 그대로다.
  future의 저장 크기와 poll의 실제 stack frame 크기는 다르다. 이 긴 owner의 요청별 future만
  `Box::pin`으로 heap에 두어 await의 큰 상태·임시값 중첩을 줄인다. 기본 thread stack·runtime·
  profile 최적화·제품 Front API는 바꾸지 않는다. 웹 Forbidden·오류 문구·매 요청 snapshot 불변,
  Reader/Controller 읽기·다른 자동화 쓰기·Local 저장/토글/실행/삭제/복원·구형 변환 보호 단언은 유지한다.
- `ci::tests::stack_overflow_keeps_the_thread_and_exit_kind_from_real_windows_output`은 원본 일곱 줄
  fixture를 7바이트 조각으로 읽고 원문/gh 앞머리·긴 꼬리·annotation 중복·끝 개행 누락·다른 FAILED와의
  공존을 검사한다. 필요한 이유: abort는 일반 FAILED를 내지 않으며, 다른 실패가 있어도 이 증거를 잃으면 안 된다.
  `stack_overflow_exit_fallback_and_successful_or_quoted_output_are_distinct`는 PID 없는 Rust 표지,
  이름 없는 Cargo 종료의 hex/unsigned/signed 코드와 성공 Python fixture·인용·잘못된 PID·일반 exit101을
  구별한다. 필요한 이유: 이름을 추측하거나 상태 문자열 인용을 실제 스택 초과로 기록하는 오탐을 막는다.
  두 keeper는 수리 전 실패했고 수리 뒤 통과한다. 기존 시험 삭제·시한/기대값 완화·고정 sleep은 없다.
- 부분 확인은 macOS common 전체 67통과/보조1ignored·자동화 82통과·의존 경계2통과,
  두 crate all-targets strict clippy와 fmt·공개 검사다. Windows는 같은 verify profile·기본 스택에서
  자동화80개가2.66초에, CI 수집기13개가0.00초에 통과했다. OS별 cfg로 macOS 두 owner가 Windows 목록에서 빠진다.
  Windows 서비스·예약 작업·Run·사용자 PATH·방화벽 기준이 모두 같고 시험 자식0개를 확인한 뒤 격리 폴더를 지웠다.
  새 두 keeper는 개별5초 미만이며 새 E2E 번호는 없다. 전체 ci-local·cross·main 병합/push는 정규 착지 차례에 수행한다.
- 참고 후보는 [Tokio test macro의 stack pinning](https://github.com/tokio-rs/tokio/blob/tokio-macros-2.7.2/tokio-macros/src/entry.rs)
  ([MIT](https://github.com/tokio-rs/tokio/blob/tokio-macros-2.7.2/LICENSE))와
  [Rust Windows stack overflow handler](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/pal/windows/stack_overflow.rs)
  ([MIT/Apache-2.0](https://github.com/rust-lang/rust/blob/master/COPYRIGHT))다.
  macro의 저장 방식과 표준 런타임의 thread/PID 진단을 대조했고 외부 코드·의존성을 가져오지 않았다.

## 고른 글 인용(t-1204)

- 웹 유닛 `selectionQuote.test.mjs` 셋은 출처 머리·터미널 줄 범위·backtick 울타리, 대화 화자·`>` 줄, 쓰던 초안 뒤 붙이기·넘침 거부를 지킨다.
  필요한 이유: 받는 agent가 읽는 글 꼴과 「쓰던 글을 지우지 않는다」는 계약이 바뀌는 회귀를 막는다.
- 웹 E2E117 `117-selection-quote`(새 번호)는 실제 host·agent·PTY의 thread pane에서 세 단계를 본다.
  Chromium 1280 바로 입력: 대화 답을 마우스로 끄는 동안 단추 없음 → 놓으면 단추 → 쓰던 초안 뒤 `인용: t-NNNN 대화 · Claude 답`,
  입력창 focus, 터미널 두 줄 끌기 → 대화 입력창 초안에 `터미널 a~b줄`(b=a+1)·울타리, 알림, PTY로 보낸 것 없음.
  Chromium 1280 입력줄: 끈 두 줄이 입력줄 초안의 쓰던 글 뒤에 붙고 입력줄 focus. WebKit 390: 고른 글 단추를 눌러 빈 초안에 붙고 입력창 focus 없음.
  필요한 이유: 선택 사건·xterm 줄 번호·두 초안 저장소·터치 focus는 브라우저와 실제 pane에서만 드러난다. 완료는 DOM 사건·PTY 출력이며 고정 sleep이 없다.
- 이 thread는 Windows에서 일해 Unix 전용 E2E 실험실(`/tmp`·UDS)과 Playwright 브라우저가 없었다. E2E117은 쓰기만 했고 실행하지 못했다.
  대신 같은 컴포넌트·xterm 5.5를 scratch 페이지로 묶어 설치된 Chrome(playwright-core 1.64.1 executablePath)으로 끌기·놓기·클릭·터치 탭,
  여러 블록·단추 줄 제외·여러 화자, xterm `getSelectionPosition` 0부터 줄과 다음 줄 첫 칸 끝을 확인했다. 웹 유닛 전체·타입 검사·빌드는 통과했다.
- 첫 착지 gate에서 이 E2E의 WebKit 390 단계가 실패했다(20초 안에 초안이 비어 있음). 단추가 고른 글을 지키려고 pointerdown을 막았는데,
  WebKit은 터치 pointerdown을 막으면 tap의 mousedown·click을 내지 않는다(Chrome 터치 탭은 click을 내 scratch 확인에서 드러나지 않았다).
  같은 fixture의 별도 진단에서 수리 전 pointer/touch 사건 뒤 click 없음을, 마우스 pointer만 막은 수리 뒤 click·초안·focus 없음을 확인했다. 시험·시한은 그대로다.

## 큰 응답 분할의 가림 검색 비용(t-1216)

- 원본 macOS CI의 `aaa_host` 전체 artifact를 확보했다. 응답 분할 owner는 큰 보고 전송까지
  1.442초, 조각 경계 가림 뒤 5.307초였으며, 잘못된 조각 단계에서 5.311초 상한 오류가 났다.
  마지막 단계 이름만으로 BadRequest 전달이 5초 걸렸다고 해석하지 않는다.
- 별도 Mac worktree의 같은 verify profile에서 가림 274ms 중 원문 8바이트 창의 집합 검색이
  203ms였다. 후보의 앞 두 바이트를 8KiB 비트 표로 거른 뒤 기존 집합으로 정확히 검사하자
  검색 16ms·가림 83ms였다. 원본 CI에는 검색 내부 계측이 없으므로 이 수치를 CI 내부 시간으로
  바꾸어 말하지 않는다. 등록·복원과 같은 잠금 안에서 표와 집합을 함께 갱신한다.
- 기존 `link::response_parts_tests::large_report_and_bad_parts_keep_requests_on_one_link`가
  Host 준비부터 실제 reader·fake 종료와 파일 정리까지 같은 5초 상한으로 검사한다.
  2*MAX_FRAME 초과 원본과 협상, hash 확인 전 원본 보존, 조각 경계를 가로지르는 비밀 가림,
  BadRequest 뒤 같은 generation의 Done, 취소·정리 단언과 데이터 크기를 유지한다.
  응답 분할·binary hash·ANSI/청크 경계의 기존 common/host owner도 함께 확인한다.
- 새 `encoded_fragments_and_nonsecret_prefix_candidates_keep_their_redaction_contract`는
  원문·JSON/URL·두 base64 형식의 각 8바이트 조각과 같은 접두부의 일반 바이너리를 확인한다.
  필요한 이유는 후보 검사로 비밀 조각을 놓치거나 접두부만 같은 내용을 과하게 가리는 회귀를
  막기 위해서다. 고정 sleep·재시도·시한 확대·기대값 완화·시험 분할은 없다.
- 참고 후보는 [Aho-Corasick의 prefilter 설계](https://github.com/BurntSushi/aho-corasick/blob/master/DESIGN.md)
  와 [memchr](https://github.com/BurntSushi/memchr)다. 둘 다
  [MIT/Unlicense](https://github.com/BurntSushi/aho-corasick/blob/master/COPYING)이며,
  후보를 거르고 정확히 확인하는 원칙을 참고했다. 기존 고정 길이 집합을 유지하고 외부 코드나
  실행 의존성을 추가하지 않는다.

### Python 성공 fixture의 gh annotation 경계

- t-1208이 착지한 main `b3fa9a08` 위에서 기존 `FailureScan`의 성공 suite 제외만 보강한다.
  gh의 여러 줄 annotation에는 첫 줄 뒤 `job\tstep\t[python exit=0] ...`처럼 시각 없이 탭이
  바로 표지 앞에 오는 줄이 있다. 성공 표지 앞의 줄 시작·공백 검사에 탭을 더한다.
  스택 초과 수집·실패 집합 선택·원본 로그 보존은 기존 코드를 그대로 쓴다.
- 기존 `successful_python_fixture_output_is_not_a_rust_failure_even_in_annotations`에 원본 형식의
  module::failed·owner·panic·느린 경고·스택 초과 fixture를 보강한다. 7바이트 청크와 마지막
  개행 누락에서도 성공한 suite만 있으면 실패가 비어야 하며, 같은 이름의 진짜 Rust 실패는 남긴다.
  기존 `failed_or_interrupted_python_suites_keep_their_failure_evidence`는 같은 gh 앞머리 조합에서
  Python exit 1/7/130/137/143을 그대로 수집하는지 확인한다.
  두 owner 보강이 필요한 이유는 성공 여부를 시험 이름으로 판정하거나 annotation 줄을 통째로
  무시해서 실제 실패를 숨기는 회귀를 막기 위해서다. 새 시험 번호·시한 변경은 없다.

## 보고서 화면 버튼 배치(t-1201)

- 기존 E2E21·101·108 전체로 목록·보관·읽음 확인/취소/실패·Reader·저장소 필터를 확인한다. 새 시험·E2E 번호는 추가하지 않는다.
- 별도 화면 확인은 같은 E2E108 fixture와 실제 host·agent를 쓰며 Chromium/WebKit × 1280/820/390/320px에서 세 탭을 촬영한다. 새로고침의 접근성 이름·툴팁·장식 SVG와 실제 목록/용량 응답, 업로드 창 열기/닫기, 필터와 두 버튼의 같은 줄 정렬·화면 안 클릭 가능·탭과 새로고침의 겹침 없음·가로 넘침 없음을 확인한다. 응답·DOM 사건으로 끝내며 고정 sleep은 쓰지 않는다.
- 웹 유닛·타입/빌드와 공개 검사를 함께 수행한다. 전체 ci-local·cross·main 병합/push는 정규 착지 차례에 수행한다. 전후 그림은 해당 thread의 library에 둔다.
- 참고는 [Primer IconButton](https://primer.style/product/components/icon-button/)([MIT](https://github.com/primer/react/blob/main/LICENSE))의 접근성 이름·툴팁과 [MUI Toolbar](https://github.com/mui/material-ui/blob/master/packages/mui-material/src/Toolbar/Toolbar.js)([MIT](https://github.com/mui/material-ui/blob/master/LICENSE))의 가로 정렬이다. 기존 공통 아이콘·버튼을 사용하고 외부 코드나 의존성은 추가하지 않는다.

## 완료·보관한 할 일의 예약 잔상(t-1200)

- 맨 위 점선 위 행은 host의 queued/check 예약이다. 원래 할 일이 완료·보관됐는데 예약이 남으면
  승인 당시 사본을 그리며 `task 없음`이 붙었다. 선택·주소·기기 저장의 잔상이 아니다.
- `task_queue::tests::completed_or_deleted_tasks_retire_pending_reservations_before_the_board_event`는
  완료 체크/명시 삭제 응답부터 예약이 finished/cancelled이고, 재시작·복원 뒤에도 재기동하지 않음을 확인한다.
  `old_archived_reservation_is_repaired_but_unknown_missing_task_keeps_its_warning`는 구판 보관 완료 예약의
  회수와 완료 근거 없는 파일 유실의 check 보존을 같은 실제 SQLite/Host 경계에서 확인한다.
  필요한 이유: UI에서만 숨기면 다른 기기·재시작·복원 때 승인한 옛 예약이 다시 나타나거나 기동할 수 있다.
  두 keeper는 수리 전 모두 Check/Finished 불일치로 실패했다.
- `tasksModel.test.mjs`의 끊긴 예약 keeper는 전체/예약 칩·검색·저장소 거르기·칩 합을 확인한다.
  필요한 이유: 진짜 연결 끊김은 경고 행을 남겨야 하며 예약 0개로 표시하거나 예약 필터에서 숨기면 안 된다.
  완료/취소 예약은 계속 제외한다. 수리 전 예약 개수가 0으로 실패했다.
- E2E116 `116-task-queue-retirement.mjs`는 격리 DB에 구판의 check 예약 두 개(완료 보관/진짜 끊김)를
  넣고 host를 다시 띄운다. 데스크톱과 별도 휴대폰 context에서 제거·경고·예약 칩·reload·완료 탭·복원을
  확인한다. 필요한 이유: host 수리와 tasks 사건이 두 기기에 전달되고 복원이 재예약하지 않는 계약을
  실제 웹 경계에서 확인해야 한다. 수리 전후 화면은 같은 fixture에서 촬영한다.
- 기존 시험·기대값·시한을 완화하거나 삭제하지 않는다. 새 검사는 사건·DOM 변경·프로세스 종료로 끝낸다.
- 전체 웹 유닛의 마지막 확인에서 기존 session-cleanup keeper가 살아 있는 session host를 보고 실패했다.
  종료 대기가 argv 변경만으로 끝날 수 있는 경계를 제거하고, 신호를 보낸 PID가 실제로 끝날 때까지 기다린다.
  추가 신호·시한 변경은 없다. 원본 실패에서 argv 변경 자체는 수집하지 못했으며 해당 분기를 특정했다고 주장하지 않는다.
  기존 keeper의 부모·분리된 writer 종료와 무관한 PID 보존 단언을 그대로 유지한다.
- 참고 후보는 [TanStack Query의 invalidate/refetch](https://github.com/TanStack/query/blob/main/packages/query-core/src/queryClient.ts)
  ([MIT](https://github.com/TanStack/query/blob/main/LICENSE))와
  [react-admin 목록 선택 정리](https://github.com/marmelab/react-admin/blob/master/packages/ra-core/src/controller/list/useListController.ts)
  ([MIT](https://github.com/marmelab/react-admin/blob/master/LICENSE.md))다.
  host 정본 갱신 후 목록이 따라가는 원칙을 대조했다. 외부 코드·의존성을 추가하지 않는다.

## 할 일 메모 추가·교체 경고·저장 전 형식 검사(t-1197)

- host `task_edit::tests::append_notes_keeps_raw_block_and_conflicts`는 CRLF·공백·첨부와 같은 기존 메모 원문·하위 task·개행 없는 끝을 보존하고, 같은 요청의 추가 뒤 제목 변경·오래된 블록 거부·메모 문법 거부를 검사한다. 필요한 이유: 전체 메모 재구성으로 원문이 손상되거나 경합 변경을 덮는 것을 막는다.
- `note_loss_detects_removed_and_replaced_lines`는 정상 추가·순서 이동 무경고·줄 감소·같은 줄 수 교체·중복 줄 감소·전체 삭제를 검사한다. 필요한 이유: 줄 수 비교만으로 놓치는 기존 내용 유실을 막는다. 기존 `save_checks`에 제목 속 thread 번호와 120자를 넘는 제목 거부를 추가한다.
- 실제 CLI·host m13 `task_note_append_warning_and_title_validation`은 짧은 d-ID와 hash로 여러 번 추가·제목 변경을 함께 저장하고, 오래된 hash·옵션 충돌·add/edit의 잘못된 제목이 파일을 바꾸지 않는지 본다. 교체의 사람이 읽는 경고와 JSON task log의 before/after·삭제 경고도 본다. 필요한 이유: clap·ID 해석·직렬화·원자 저장·DB 기록·화면 출력까지의 연결 계약을 유닛만으로 지킬 수 없기 때문이다.
- 기존 E2E46에 실제 웹 메모 추가·첨부 보존·유실 무경고, 제목 저장 거부·원문 보존, 교체 뒤 줄 수 경고와 task log 원문, 저장 거부 때 추가 입력 보존을 네 단계로 더한다. 필요한 이유: 저장 응답 경고가 화면에서 사라지거나 추가 입력이 기존 메모를 교체하는 회귀를 막는다. 고정 sleep·재시도·시한 확대·기대값 완화는 없다. 새 E2E 번호는 만들지 않는다.
- 검증 범위: 관련 host 유닛과 m13, proto 생성 일치, 웹 유닛·타입/빌드·E2E46, 의존 경계·strict clippy·fmt·공개 검사. 전체 ci-local·cross와 main 병합/push는 정규 착지 차례에서 수행한다. 참고 후보와 호환은 plan §8.518에 적는다.

## E2E03·105 비동기 관측과 CI 웹 실패 수집(t-1184)

- 원본 [macOS web-e2e job](https://github.com/cLazyZombie/aaa/actions/runs/40477028804/job/122097672768)
  (main `d82e3fee`)의 전체 job 로그·e2e.log·run-timings.jsonl·artifact 목록을 확보했다.
  원본 artifact에는 두 실패의 PNG·상태 JSON·host 전체 로그가 없다. job 마지막에는 각 실험실의
  host/agent 로그 꼬리가 있어 별도로 보존했다. 이 꼬리만으로 xterm 내부 호출까지 관측했다고 하지 않는다.
- E03의 시작은 viewTop/viewBase=169, parsedTop/parsedBase=171, 첫 줄 s-172, 30행,
  nativeTop/bottom=3249였다. PageUp→142→PageDown→171 뒤 첫 줄과 nativeTop은 그대로 복귀했다.
  snapshot ACK는 이후 resize state·본문 render·native viewport의 동시 완료를 보장하지 않는다.
  `pageScrollStart`는 크기 응답·파싱 base·render base·실제 행 높이에 따른 픽셀 위치가 맞는 순간의
  기준을 한 번에 반환한다. 완료 단언의 옛 base 169를 기대값 171로 바꾸거나 버퍼 검사를 없애지 않는다.
- E105 chromium-390의 원본 activeElement는 body, 사건은 select(3..8) 한 건이다.
  같은 사건을 기존 코드의 실제 Chromium에서 재현했다. 시험의 setSelectionRange가 돌아와도 native select는
  뒤에 올 수 있어 터치 전환용 기록 초기화 뒤 섞였다. `selectInputAndWait`는 선택 전 구독하고 실제 사건까지
  끝낸다. WebKit의 같은 범위 무사건도 고려한다. 사건 배열의 빈 배열 단언·초안·양방향 선택·복귀 입력·
  보관 만료·자동 focus 금지·숨은 pane inert·PTY 입력 0회는 모두 유지한다.
- 기존 focus 단계는 네 엔진/폭 조합을 각 단계로 나눴다(Chromium/WebKit × 1280/390).
  같은 fixture와 계약을 그대로 검사하며 관련 스크롤 단계 3.2초, focus 각 단계 약 1~2초였다.
  파일 전체 실행 시간과 개별 계약 시간을 구별한다. 기존 E03 키 막대와 E105 MRU·앱 화면의
  다른 긴 단계는 이번 수리의 변경 대상이 아니며 전체 5초 감사 대상에 남는다.
- 새 `e2e-observation.test.mjs` 두 keeper가 필요한 이유는 옛 render와 현재 버퍼가 갈리는 순서,
  선택 메서드 반환과 뒤따르는 select 전달을 사건 gate로 소유해 같은 준비 경쟁을 막기 위해서다.
  기존 실제 upstream xterm PageUp/PageDown·사용자 scroll 유닛과 실제 host·PTY E03 계약도 유지한다.
  새 대기의 실패 상한은 5초이며 고정 sleep·실패 재시도·시한 확대·단언 완화가 없다.
- t-1182·t-1183의 선행 수집/annotation 변경 위에서 `FailureScan`이 원문·gh job/step/time·
  단계/annotation 앞머리의 FAIL 줄을 단계 이름으로 수집한다. 오류/DOM은 신원에서 뺀다.
  이름이 없으면 suite-end의 양수 failed/jobs를 보존한다. 새 Rust keeper 두 개는 7바이트 청크·
  긴 꼬리·한글·개행 없는 끝·annotation 중복·실제 Rust 실패 공존·요약 fallback을 지킨다.
  필요한 이유는 실패한 웹 suite가 failures=[]로 분류되거나 성공 fixture가 진짜 실패로 섞이지 않게 하기 위해서다.
  기존 성공 Python fixture·완료한 느린 시험 회귀도 유지한다. 수리 전 두 keeper가 모두 실패했다.
- 두 spec은 실패할 때 페이지 닫기/진단용 resize 전에 JSON·PNG·host/agent 로그를 diagnostics에 둔다.
  기존 harness가 e2e-failures로 복사한다. 임시 의도한 실패 한 번에서 종료 1과 다섯 파일의 실제 복사를 확인했다.
- 부분 검증: 웹 유닛 626개·타입/빌드, E03/E105 전체, common 유닛·strict clippy,
  의존 경계·fmt·공개 검사다. 전체 ci-local·cross·main 병합/push는 정규 착지 회차에서 수행한다.
- 참고 후보는 [xterm 5.5.0 Viewport](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)
  ([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)),
  [VS Code terminal 키 처리](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/terminalInstance.ts)
  ([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt)),
  [Playwright focus](https://github.com/microsoft/playwright/blob/main/tests/page/page-focus.spec.ts)와
  [scroll 시험](https://github.com/microsoft/playwright/blob/main/tests/page/page-click-scroll.spec.ts)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))다.
  native viewport와 버퍼의 비동기 경계·앱 키 처리의 shell 입력 차단·실제 focus/scroll 사건 검사를 대조했다.
  외부 코드·의존성을 추가하지 않는다.

## 바이너리 보고 사본 원본 보존(t-1180)

- `report_copy::` 두 유닛은 비밀값 조각을 넣은 실행 파일·압축·그림·PDF·음원, 텍스트처럼 보이는
  바이너리 확장자, 확장자가 틀리거나 없는 NUL/비UTF8 파일의 SHA-256·바이트 보존과 내용 없는 경고를
  확인한다. 파일 앞 8KiB 뒤의 이진 내용도 검사한다. Markdown·로그·HTML·JSON·SVG·확장자 없는
  한글/ANSI 텍스트의 가림도 지킨다. 필요한 이유: 파일 형식이나 prefix만으로 판정해 첨부가 다시
  손상되거나 텍스트 가림이 사라지는 회귀를 막는다.
- 기존 `link::post_tests::stable_report_transfer_verifies_before_masking_and_keeps_a_verified_copy`는
  실제 host·agent link의 조각 경계 비밀값과 원본 검증을 유지하며 최신 사본·불변 r 판의 바이너리 hash를
  비교한다. 보고 본문 가림·단계와 파일 단위 inbox 경고의 원문 비노출도 확인한다. 수리 전 같은 입력의
  hash 불일치 실패를 확인했다. 기존 m13
  `secret_k0::remote_report_with_registered_auth_bytes_copies_without_false_hash_failure`에도
  실제 TCP+TLS로 전송한 실행 파일의 최신/불변 사본 hash 비교를 더하고 로그 가림·재시작 계약을 유지한다.
  새 E2E 번호·고정 sleep·재시도·시한 확대·기대값 완화는 없다.
- 참고는 고정 소스의 herdr-projects v0.2.35 `src/thread.rs`(MIT)의 rsync 원본 복사,
  herdr-web-ui `aa624cb` `server/file-view.ts`(MIT)의 종류 판정과 `Bun.file` 바이트 전송,
  herdr v0.9.5 `src/server/clipboard_image.rs`(Apache-2.0)의 이미지 바이트 보존이다.
  별도 코드·의존성을 들여오지 않는다. 전체 내용 판정과 조각 일치 경고는 aaa의 가림 경계에 맞춘 차이다.
- 부분 검증: `cargo test --profile verify --locked -p aaa-host --lib -- report_copy:: link::post_tests::`,
  `cargo test --profile verify --locked -p aaa-common --lib redact::`, 위 m13 owner와
  `cargo test --profile verify --locked -p aaa --test deps`, 두 crate strict clippy·fmt·공개 검사다.
  전체 ci-local·cross·main 병합·push는 정규 착지 차례에서 수행한다.

## Rust shard 중단 기록과 완료 경고 구별(t-1182)

- [원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40470706270/job/122076871066)의
  다섯 60초 경고는 모두 같은 test 단계에서 `... ok`로 끝났다. rollout 세 shard는 각각
  101.11/115.73/133.77초에 exit=0, coordinator 배포와 configured preparation은
  m13-5의 50개 완료에 포함된다. 여섯 m13 shard는 08:23:22 UTC까지 모두 exit=0이다.
  이 다섯 시험이 15분 중단 시점에도 정지해 있었다는 분류는 원문과 다르다.
- test 단계는 08:12:27 시작, Cargo 준비가 08:16:36 완료(4분06초), 마지막 binary가
  08:26:13 완료, 08:27:40에 15분 제한으로 중단됐다. 준비한 25개 binary를 m13 여섯 개로
  나눈 30개 실행은 전부 exit=0이다. 원본 runner 순서상 결과 없는 Rust job은 마지막 doctest다.
  m13-4 이후 완료 간격은 다음 binary 실행시간과 거의 같아 사실상 직렬이었다.
  원본에는 CPU/load·shard plan/summary·doctest 출력·진행 중 PID·host/agent 로그가 없다.
  artifact 하나는 성공한 aaa_common 원문뿐이다. doctest의 컴파일/실행/자식 종료 중 어느 대기인지,
  또는 Python suite 대기가 함께 남았는지는 소급 확정하지 않는다. 재시도나 제한 확대는 하지 않는다.
- `scripts/rust_tests.py`는 준비 Cargo, m13 목록, 각 binary, doctest의 기동 직후
  `START`와 PID·로그를 출력하고 `events.jsonl`에 시작/종료 사건을 쓴다.
  `state.json`은 준비/목록/실행 단계, queued/running/completed/interrupted/not_started,
  실행 중 직접 자식의 PID·명령·cwd·로그·종료 대기와 완료 결과를 같은 폴더 rename으로 저장한다.
  `summary.json`도 시작부터 매 사건마다 갱신하므로 정상 종료까지 기다리지 않는다.
  libtest 원문은 자식이 직접 파일에 쓰고, Cargo 준비의 JSON과 stderr는 별도 보존한다.
- SIGINT/TERM handler는 신호 번호만 기록한다. worker는 중단 당시 직접 자식과 그 자손의
  PID/PPID·실행 파일·OS 상태/대기 채널을 기록하고, Popen으로 직접 소유한 자식 하나만
  terminate→종료 대기→필요한 kill/wait로 회수한다. 자손 목록은 진단용이며 종료 대상이 아니다.
  인자·환경은 process snapshot에 넣지 않는다. 중단 뒤 대기 shard는 시작하지 않고,
  도구가 신호를 받아 0으로 끝나도 runner는 130/143으로 실패한다. SIGKILL은 마지막 원자 상태와
  이미 쓴 원문/사건만 남길 수 있다. 자손/독립 lab 정리와 host/agent 파일 수집까지 완료했다고 주장하지 않는다.
- CI는 test 직후 성공/실패/취소에도 `test.log`, `python.log`, `rust/`를 OS별 artifact로 보존한다.
  뒤의 설치 시험보다 먼저 업로드하며, 단계의 기존 15분 제한과 실패 판정은 그대로다.
- `FailureScan`은 60초 경고 뒤 완료한 이름을 기억해 annotation의 경고 재출력을 정지 시험에서
  제외한다. 완료는 경고만 없애고 같은 이름의 실제 FAILED·panic은 유지한다. 개별 시간 표지가 붙은
  FAILED도 실제 실패다. 완료 원문이 없는 경고는 보수적으로 남긴다.
- 새 Python keeper 다섯 개는 socket 사건으로 실제 자식을 붙잡고 준비/목록/실행/doctest에서
  SIGINT/TERM을 보낸다. 필요한 이유: 시작 기록의 선행, 현재 PID·중단 snapshot·130/143,
  미시작 shard·기존 실패 보존·TERM 무시 자식 회수와 runner 밖 프로세스 생존은 모의 Popen으로 확인할 수 없기 때문이다.
  새 Rust keeper는 7바이트 청크·100개 선행 완료·annotation 반복·남은 대기·같은 이름의 실제 실패를
  지킨다. 필요한 이유: 완료 이름 수집의 상한이나 완료 처리로 실제 실패를 숨기는 회귀를 막기 위해서다.
  모두 고정 sleep 없이 사건·자식 종료로 끝나고 개별 5초 미만이다. 기존 시험 삭제/시한 변경은 없다.
- 부분 검증은 `python3 -m unittest scripts.test_rust_tests scripts.test_ci_local -v`,
  CI Python 전체 suite, `cargo test --profile verify --locked -p aaa-common --lib ci::`,
  같은 crate의 all-targets strict clippy, fmt와 공개 검사다. 전체 ci-local·cross는 정규 착지 때 수행한다.
- 설계 전 후보는 [cargo-nextest의 현재 실행과 완료 상태 구분](https://nexte.st/docs/reporting/)
  ([MIT](https://github.com/nextest-rs/nextest/blob/main/LICENSE-MIT)),
  [libtest-mimic의 실행/결과 처리](https://github.com/LukasKalbertodt/libtest-mimic/blob/master/src/lib.rs)
  ([MIT](https://github.com/LukasKalbertodt/libtest-mimic/blob/master/LICENSE-MIT)),
  [pytest의 setup/call/teardown 보고](https://github.com/pytest-dev/pytest/blob/main/src/_pytest/runner.py)
  ([MIT](https://github.com/pytest-dev/pytest/blob/main/LICENSE))다. 시작·완료 사건과 단계/최종 결과 구분을
  참고하며 코드를 복사하거나 runner 의존성을 바꾸지 않는다. CPU admission/분할 정책은 유지한다.

## Windows host의 원격 후보 경로(t-1183)

- [원본 Windows job](https://github.com/cLazyZombie/aaa/actions/runs/40472958256/job/122084226189)의
  두 rollout 시험은 모두 `BadRequest`·`ID·절대 경로·SHA-256·커밋 형식을 확인한다`로 실패했다.
  같은 경로와 단언의 수정 전 Windows 실기에서 두 오류를 재현했다. 새 경로 owner도 `/candidate/aaa`에서
  실패했다. 입력을 다른 경로로 바꾸거나 시험의 단언·시한을 완화하지 않는다.
- `cargo test --profile verify --locked -p aaa-host --lib rollout::`는 기존 rollout 16개와 새
  `candidate_paths_use_remote_syntax_and_keep_local_host_rules`를 실행한다. 새 시험이 필요한 이유는
  Windows host에서 원격 POSIX 절대 경로를 받아야 하고, 드라이브 절대 경로·~/ 지원과
  상대 경로·C:relative 거절 및 host 로컬 경로 규칙이 함께 유지되어야 하기 때문이다.
  로컬 기대값은 OS별 계약을 명시하며 같은 Path 함수에서 만들지 않는다.
- `python3 -m unittest -v scripts.test_ci_local`은 실제 CI 셸의 annotation과 순수 발췌 owner를 검사한다.
  새 두 시험이 필요한 이유는 긴 역추적 뒤에도 panic과 첫 ErrorBody code/message가 함께 남고,
  runner 환경의 인증값이 가려지며 기존 퍼센트 escape·원본 로그·실패 종료를 보존해야 하기 때문이다.
  annotation의 최근 8개 실패 묶음·꼬리 5줄 상한을 유지한다. 실패 시험 이름/집합 판정은 바꾸지 않는다.
- 부분 검증은 macOS rollout·경로 owner·aaa-host strict clippy·의존성 경계·fmt·공개 검사와
  Windows 격리 폴더에서의 rollout·최종 경로 owner·Python 순수 owner다.
  Windows 서비스·예약 작업·Run·사용자 PATH·방화벽 기준을 시험 뒤 대조하고 시험 폴더를 정리한다.
  전체 ci-local·cross·main 통합과 push는 정규 착지 차례에서 수행한다.
- 참고는 [std::path의 OS별 절대 경로](https://doc.rust-lang.org/std/path/struct.Path.html#method.is_absolute),
  [camino의 같은 OS 규칙](https://docs.rs/camino/latest/camino/struct.Utf8Path.html#method.is_absolute),
  [typed-path의 명시적 Unix/Windows 문법](https://github.com/chipsenkbeil/typed-path/blob/main/src/typed/non_utf8/path.rs)이다.
  두 crate는 MIT OR Apache-2.0다. 문법을 분리하는 원칙을 참고하고 코드·의존성은 추가하지 않는다.


## 삭제 뒤 TASKS 기억과 CI fixture 출력(t-1172)

- [원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40462675441/job/122050697112)은
  m13 `task_memory_forgets_deleted_project`에서 새 일 하나의 `add / file / id=2`를 남겼다.
  같은 로그의 Python suite는 `Ran 62 tests`·`OK (skipped=2)`다. 자식 fixture의 `owner`·`module::failed`
  출력이 성공한 suite 안과 실패 annotation에 반복되므로 실제 Rust 실패와 구분한다.
- 실제 Host/SQLite/프로젝트 파일로 세 순서를 직접 실행한다. 재생성한 채운 파일의 첫 관측은 기록 없음,
  재생성한 빈 파일을 먼저 관측한 뒤 편집하면 정상 `add / file` 하나,
  삭제 뒤 늦은 `tasks_pass`가 도착한 뒤 재생성하면 기록 없음이다. 수리 전 마지막 순서는
  원본과 같은 `add / file / id=2`로 실패했고 앞의 두 순서는 통과했다.
  원본에는 개별 tick 순서가 없어 당시 직접 원인을 둘 중 하나로 소급 확정하지 않는다.
- `tasks_pass`는 TASKS 파일 lock 아래서 PROJECT.md가 살아 있는지 확인한다. 프로젝트 삭제는
  같은 lock으로 rename·DB forget·기억 제거를 묶고 비동기 agent 정리 전에 놓는다.
  없는 프로젝트를 빈 파일로 기억하는 재삽입과 삭제 전 관측이 삭제 후 기록되는 경쟁을 막는다.
- 기존 m13 owner를 `aaa-host`의 `lifecycle::task_memory_tests::task_memory_forgets_deleted_project`로
  대체했다. CLI·agent·Git 기동 대신 실제 제품 메서드의 관측 순서를 소유해 준비 경쟁을 없앤다.
  기존 직접 편집 기록·삭제·동일 이름 재생성·첫 관측 무기록을 유지하고 이후 편집 기록도 확인한다.
  추가 두 keeper가 필요한 이유는 정상 빈 파일 관측과 삭제된 프로젝트의 늦은 관측을 구별하기 위해서다.
  고정 sleep·재시도·시한 확대·기대값 완화는 없다. 세 시험 합계는 약 0.4초다.
- ci-local은 Python 종료를 기다린 뒤 모든 출력 줄에 `[python exit=N]`을 붙인다. 원본 python.log는
  그대로 남긴다. 수집기는 exit=0 표지가 있는 줄만 제외하고, 같은 이름의 실제 Rust 실패·비정상/중단된
  Python suite·표지가 없는 옛 로그는 보존한다. annotation 재출력도 같은 표지를 유지한다.
  마지막 개행이 없는 자식 출력도 다음 Rust 실패와 붙지 않게 줄을 끝낸다.
- 새 `ci::tests` 두 keeper는 gh 앞머리·병렬 단계 표지·7바이트 청크·annotation 중복과 같은 시험 이름의
  실제 실패 보존, 비정상 종료·마지막 개행 없는 로그를 검사한다. 필요한 이유는 이름 제외나 전체 실패
  집합 초기화가 실제 회귀를 숨기지 않게 하기 위해서다. 기존 CI 셸 keeper도 성공/실패 suite의 실제
  종료 코드·원문 로그·마지막 개행 경계를 확인한다. 새 E2E 번호나 제품 의존성은 없다.
- 참고 후보는 [Tokio Notify의 알림과 완료 구별](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))와
  [CPython unittest runner의 바깥 결과](https://github.com/python/cpython/blob/3.13/Lib/unittest/runner.py)
  ([PSF-2.0](https://github.com/python/cpython/blob/3.13/LICENSE))다.
  완료 순서를 직접 소유하고 자식 출력과 바깥 suite 판정을 구별하는 원칙을 참고했다. 코드는 복사하지 않는다.
- 부분 검증은 `cargo test --profile verify --locked -p aaa-host --lib -- lifecycle:: task_edit:: file_rev:: tasks::`,
  `cargo test --profile verify --locked -p aaa-common --lib ci::`, CI Python 전체 suite,
  두 crate의 all-targets strict clippy, `cargo test --profile verify --locked -p aaa --test deps`,
  기존 m13의 프로젝트 수명/할 일 편집 keeper와 fmt·공개 검사다.
  전체 ci-local·cross·main 병합과 push는 정규 착지 차례에서 수행한다.

## 배포 시작 전 중단 기록(t-1178, plan §8.511)

- `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib rollout::`: 기존 14개 owner와 새 두 회귀를 실행한다. `disconnected_rollout_source_is_durable_and_same_id_never_restarts_it`는 끊긴 빌드 머신의 stopped 정본·status·같은 ID 조회/다른 요청 거부·단일 종료 알림을 지킨다. `all_targets_keep_disconnected_machine_and_stop_before_any_agent_request`는 연결된 소스와 끊긴 다른 대상에서 all 명단·대상별 하위 오류·웹 원문 가림·작업/감시 미실행·다음 시작 가능을 검사한다. 필요한 이유: CLI가 ID를 먼저 표시한 뒤 시작 검사에서 기록 없이 빠지는 회귀와 끊긴 대상을 몰래 제외하는 회귀를 막기 위해서다.
- `nice -n 15 cargo test --profile verify --locked -p aaa --test deploy_start -- --nocapture`: 실제 격리 host·로컬 agent·사람 PTY CLI를 쓴다. 시험 DB에 끊긴 빌드 머신을 등록하고 존재하지 않는 후보 경로로 요청하므로 설치·서비스 호출을 하지 않는다. 접수 전/중단 안내·JSON 정본·status --wait의 실패 종료·형식 오류의 거절 안내·host 재시작 뒤 같은 정본·원인 로그를 확인한다. 필요한 이유: host 유닛만으로는 CLI 안내·종료와 실제 DB 재개를 확인하지 못한다. 사건과 자식 종료로 끝내며 새 안전 상한은 5초다.
- 전체 ci-local·cross·착지는 정규 착지 차례에서 수행한다. 재시도·시한 확대·기대값 완화·운영 배포는 없다.

## E2E16 누른 사이 resize의 링크 소실(t-1167)

- [원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40450356721/job/122011375425)의
  전체 로그·artifact 목록·실패 JSON/PNG를 확보했다. 설정 go/popstate 뒤 down이 도착하고,
  크기 맞춤·행 변경·링크 장식 제거 뒤 같은 좌표에 up이 도착하지만 파일 요청·pushState·viewer mount가 없다.
  ws1006은 앞선 파일 열기 세 번보다 이전에 복구했다. 원본에는 xterm 내부 호출 기록이 없으므로
  당시 메서드 호출까지 관측한 것으로 쓰지 않는다.
- 기존 t-1151 패치는 새 응답 객체와의 동일 대상 비교만 다룬다. 실제 upstream 생성자의 resize listener는
  현재 링크와 render listener도 지우므로 이후 PTY 재그림으로 복구되지 않는다. 기존 E2E16 wrap 재열기에
  down → 실제 창 높이 한 줄 증가 → 크기 응답·그리드 반영 → 같은 행 PTY 재그림 → up 순서를 고정하면
  이전 수리판에서도 같은 viewer=false·preview=false·rows=0·20초 실패가 재현된다.
- `xterm-5.5.0-link-press.patch`는 누름 중 resize가 있었으면 up의 실제 칸에서 provider를 새로 조회한다.
  그 사용자 사건 안의 동기 응답에만 기존 provider·대상·전체 범위와 놓은 자리 비교를 적용하며 누름을
  먼저 소비한다. 늦은 응답으로 열기를 재생하지 않는다. 앱의 왼쪽 단추·선택 없음·현재 버퍼 재검증을 유지한다.
- 새 `xterm-link-resize.test.mjs`는 실제 upstream 클래스의 생성자·resize·render listener 해제·provider·up을
  실행해 같은 대상 한 번, 바뀐 대상/범위/provider·범위 밖·mouseleave·늦은 응답·추가 up 거부와 dispose를 검사한다.
  필요한 이유: up 메서드만 추출하던 기존 유닛은 resize가 링크와 감시를 함께 없애는 경계를 검사하지 못했다.
  기존 재그리기 유닛과 E2E 번호는 유지한다. 새 유닛과 추가 사건 대기의 실패 상한은 5초 이하다.
- 기존 E2E16의 설정 기록 gate·wrap/줄 이동/가상화/강조 생략/큰 파일/빈 파일/라이선스·기기 조합을 유지한다.
  resize 뒤 112열·한 줄 증가·링크 좌표 불변을 확인하며 실제 PTY 재그림 계약도 보존한다.
  재시도·상한 확대·기대값 완화·고정 sleep·대체 클릭은 없다. 부분 검증은 웹 전체 유닛·타입/번들,
  고정한 실행본의 E2E16 전체와 E2E03 드래그/복사/제자리 클릭이다. 전체 ci-local·착지는 정규 차례에서 수행한다.
- 참고 후보는 [xterm 5.5.0 Linkifier](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Linkifier.ts)
  ([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))와
  [Playwright 입력](https://playwright.dev/docs/input)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))이다.
  resize의 링크 수명과 실제 down/up 사건을 대조하며 외부 코드·제품 의존성은 추가하지 않는다.

## E2E54 속성 저장의 변경 전 기준(t-1168)

- `save(want, change)`가 선택 동작 전에 파일 hash를 읽는다. 네 저장 호출 모두 같은 경계를 사용하며,
  원하는 원문 줄·변경 전과 다른 판·실제 파일과 같은 DOM hash·saving 해제·저장 단추 없음과
  모델/머신 독립 변경·칩·바로 시작 잠금·저장소와 iPhone 시작 계약을 유지한다. 제품 코드는 바꾸지 않는다.
- 기존 머신 선택의 callback 안에서 실제 host 파일과 DOM의 저장 완료를 먼저 관측한 뒤 반환한다.
  필요한 이유: 선택 RPC의 반환보다 저장이 먼저 끝나는 정상 순서에서 기준을 잃는 회귀를 결정적으로 잡기 위해서다.
  같은 대조에서 옛 순서는 파일/DOM/응답의 hash가 같고 saving=false인데도 기존20초 「새 판」에서 실패했다.
  변경 전 기준을 먼저 읽는 수리는 같은 실제 host 대조를 포함한 E54 전체를 통과했다.
  고정 sleep·재시도·시한 확대·기대값 완화·새 E2E 번호·시험 삭제는 없다.
- `task-save-trace.mjs`는 E54의 합성 자료만 기록한다. 호출 번호·old/expected/file/DOM hash·saving·폼 객체의 수명,
  task_edit/tasks 요청·응답 헤더와 JSON 본문 완료·tasks 소켓 사건을 연결한다.
  시간은 Node 관측 도착 순서이며, DOM에는 브라우저 관측 시각도 남긴다. 파일 감시는 쓰기 발생의 정확한 시각이 아니다.
  본문·URL·쿠키는 남기지 않으며 제품 요청·응답·사건을 바꾸지 않는다.
  출력은 `AAA_E2E_ARTIFACTS` 또는 해당 실험실의 `task-save-<pid>.jsonl`이다.
- 원본 main26c98989의 [CI 로그와 artifact](https://github.com/cLazyZombie/aaa/actions/runs/40450356721/job/122011375425)에는
  어느 저장 호출인지·파일/DOM hash·saving 기록과 E54 화면이 없다. 이 경쟁의 존재와 결정적 재현은 확정하지만,
  원래 CI 실패의 개별 호출·직접 원인이나 ws1006과의 인과는 소급 확정하지 않는다.
- 부분 확인은 저장소 고정 Playwright1.64.1과 공용 브라우저 캐시로
  `NODE_PATH=<격리 node_modules> nice -n 15 node web/e2e/web.e2e.mjs --spec 54 --workers 1` 및 `cd web && npm test`다.
  전체 ci-local·cross·공개 검사와 main 착지는 정규 착지 차례에서 수행한다.
- 참고 후보는 [Playwright 사건을 동작 전에 구독](https://playwright.dev/docs/events#waiting-for-event)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와
  [React controlled select의 onChange와 상태 반영](https://react.dev/reference/react-dom/components/select#controlling-a-select-box-with-a-state-variable)
  ([MIT](https://github.com/facebook/react/blob/main/LICENSE))다. 동작 전 관측과 상태/비동기 저장의 구별을 따르며
  외부 코드·의존성을 추가하지 않는다.


## E2E12의 문서 식별과 본문 완료(t-1117)

- 관측기는 `timeOrigin`을 반복 환산해 문서 키로 비교하지 않고, 실제 Document에 한 번 붙인
  표지를 HTTP 헤더·완료 대기·문서 교체 취소에서 함께 읽는다. hash 이동은 표지를 유지하고,
  새 Document는 같은 시계 값에서도 다른 표지를 받는다. 완료 대기의 키·목표 개수와
  수집 키별 완료 개수도 원문에 남긴다.
- `threadPageTrace.test.mjs`는 직렬화한 실제 init/getter와 HTTP 헤더를 대조하고,
  같은 문서의 시계 변동·다른 문서의 같은 시계·이전 문서/미완료 본문 배제를 검사한다.
  필요한 이유는 두 완전 목록의 본문이 모두 있는데도 다른 문서 키를 기다리는 정지를 막기 위해서다.
- 제품 `threadsSync`·초기 소속 숨김·재연결 중 기존 분류는 유지한다. E12의 실제 host/두 브라우저,
  두 완전 목록·2505건·차례·중복·실제 DOM, 기존20초 상한과 본문 오류/문서 취소 단언도 유지한다.
  최초 실패 원문을 남기고 수리 전 같은 E12를 재실행하지 않는다. 전체 확인은 새 착지 차례에서 한다.
- 참고는 [Playwright init의 문서별 실행](https://playwright.dev/docs/api/class-page#page-add-init-script)과
  [Puppeteer의 문서 수명 구별](https://github.com/puppeteer/puppeteer/blob/main/packages/puppeteer-core/src/cdp/LifecycleWatcher.ts)
  (각 Apache-2.0)이다. 문서 수명을 시계 수치와 분리하는 원칙을 따르며 코드·의존성은 복사하지 않는다.

## 누출 검사의 모든 byte 조각 보존(t-1117)

- m13 `secret_k0::hits`는 같은 파일의 byte를 검색어마다 다시 훑는 대신 Aho-Corasick의 겹침 검색을 쓴다.
  기존 모든 원문·8바이트 조각·base64·URL·JSON 변형과 파일/제외 경로를 유지하고,
  동일 byte의 별도 라벨 및 각 검색어의 첫 byte 위치를 원래 순서로 보고한다.
- `byte_scan_preserves_overlaps_duplicate_labels_first_offsets_and_binary_files`는 실제 파일에서
  겹침·중복 라벨·첫 위치·NUL/비UTF8·제외 경로·빈 검색 목록을 지킨다. 필요한 이유는
  빠른 검색이 겹친 누출을 버리거나 문자열 변환으로 원문 byte를 바꾸는 회귀를 막기 위해서다.
  기존 계정 전달/회전/재개·이미지 전송·원장 비노출 owner의 입력과 단언은 그대로 둔다.
- 후보는 [Aho-Corasick의 Standard 겹침 검색](https://docs.rs/aho-corasick/1.1.5/aho_corasick/enum.MatchKind.html)과
  [memchr의 단일 검색어 Finder](https://docs.rs/memchr/2.8.3/memchr/memmem/struct.Finder.html)다.
  두 패키지는 `Unlicense OR MIT`이며 이미 lock에 있던 Aho-Corasick을 시험 의존성으로 직접 연결한다.
  외부 소스를 복사하지 않고 API를 사용하며 판을 갱신하지 않는다.
- 부분 확인은 해당 새 m13 byte owner와 기존 계정/이미지 owner를 각각 nice15로 실행한다.
  검색어/파일 축소·실패 재실행·시한 확대·기대값 완화 없이, 전체 효과는 같은 조건의 전후 계측으로 판단한다.

## 이미지 owner의 준비와 완료 사건(t-1117)

- 이미지 fixture는 처음부터 이미지 설정으로 host를 열고 실제 로컬·원격 agent와 프로젝트를 준비한다.
  사용하지 않는 Git 저장소와 시작 직후 버릴 host를 만들지 않는다. 실제 중간 host 재시작은 유지한다.
  `start_image_host`와 `restart_host`는 공통 기동 helper로 같은 fixture 설정을 이어 간다.
- 정적인 fake 입력창은 실제 준비 사건 뒤 즉시 brief를 받는다. 기본 3초 quiet 정책은 바꾸지 않고
  해당 정책의 기존 brief owner와 실제 worker 입력/출력 검사를 유지한다.
- 이미지 CLI의 종료 코드는 파일 close·rename 뒤 고유한 실제 PTY 출력으로 알린다.
  `probe_completion_is_published_only_after_exit_code_is_written`의 파일 생성/쓰기 사이 gate와
  원문·종료 코드·원자 rename 단언에 완료 출력 단언을 더한다. 같은 상한 안에서 사건으로 끝낸다.
- 기존 `host_account_images_transfer_to_local_and_remote_threads_without_credentials_or_duplicates`는
  생성·권한·계정 선택·같은 ID·실패·가림을 지킨다.
  `host_and_thread_restart_preserve_generated_images_and_coordinator_scope`는 실제 생성부터 다시 수행한 뒤
  같은 주소의 host·같은 thread 재시작, 기존 파일 fetch·목록 범위·forget을 지킨다.
  두 번째 owner도 상태를 미리 넣지 않고 실제 helper 시도 4회를 수행한다.
  기존 55개 단언 식과 544KiB PNG·모든 canary/변형을 유지한다.
- 독립 owner를 준비하며 반복되는 실제 생성 비용도 합산해서 보고한다. 이름 분리만으로 속도 개선을 주장하지 않는다.

## E2E103 lookup 정지의 protocol 원문 보존(t-1147)

- `retention-protocol.mjs`는 기존 Chromium 주 frame session의 DOM/Runtime 호출 요약에 `protocol` 원문 객체를 더한다. 요청 params, 응답 result/error, context 생성·소멸의 원문으로 objectId·backendNodeId·executionContextId와 호출 ID를 연결한다. 기록은 합성 E103의 count/lookup/commit·실패 진단으로 한정하고 Network·console 사건과 관측 밖 새 호출은 제외한다. 운영 페이지나 임의 evaluate를 기록하는 도구로 사용하지 않는다. 알려진 합성 DOM/함수만 읽는 이 범위와 달리 임의 Runtime 호출은 비밀값을 반환할 수 있다.
- 최초 오류를 기록한 다음 추가 evaluate 전에 `failure-boundary`와 `pending-at-failure`를 즉시 저장한다. 페이지 종료의 `pending-at-close`와 별개이며, 늦은 응답이 와도 최초 경계를 덮어쓰지 않는다. 원문은 독립 snapshot으로 메모리에 쌓고 단계 해제/실패/종료에서 파일로 쓴다. 원문 저장 실패도 `protocol-failure`로 남기며 최초 API 오류를 유지한다. 호출·Promise·응답과 기존 locator30초/commit20초/추가 진단5초는 바꾸지 않는다.
- `nice -n 15 node --test web/test/retentionProtocol.test.mjs web/test/retentionFailure.test.mjs`는 원문/오류 보존·실패 당시 pending·늦은 응답·페이지별 격리·공유 logger 복구·추가 진단 전 snapshot·저장 실패에도 최초 오류 보존을 검사한다. 필요한 이유: 종료 뒤의 pending만 보면 최초 정지와 추가 진단을 구분할 수 없고, 요약만으로 handle/context 연결을 복원할 수 없다.
- `NODE_PATH=<격리 node_modules> nice -n 15 node --test web/e2e/retention-protocol.test.mjs`는 저장소 고정 Playwright1.64.1과 공용 캐시의 실제 headless Chromium에서 정상 locator/adoption 및 lookup-element/DOM.resolveNode 응답 보류를 확인한다. 각 시험의 실패 상한은5초다. host·운영 페이지·고정 sleep 없이 실제 응답 도착과 page close 사건으로 끝난다. `AAA_CHAT_DIAGNOSTICS=<폴더>`를 주면 각 대조의 원문을 보존한다. 필요한 이유: 유닛만으로 확인할 수 없는 Playwright 내부 logger와 실제 Chromium 핸들 전달 경계를 검증한다.
- 응답 보류는 관측기를 검증하는 통제 대조이며 원래78회차의 원인 재현이 아니다. 원본 로그에는 visible locator 뒤30초 오류와 Chromium1280/coordinator/trial2/page3 commit까지 있지만 실패 당시 DOM/Runtime 원문은 없다. 정확한 정지 호출과 원인은 여전히 재발 원자료로 확인해야 한다. locator 우회·시한 연장·시험 삭제는 하지 않는다.
- OSS 후보는 [Playwright CRConnection](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/server/chromium/crConnection.ts)와 [Puppeteer Connection](https://github.com/puppeteer/puppeteer/blob/main/packages/puppeteer-core/src/cdp/Connection.ts)(각 Apache-2.0)이다. 송수신 ID/session 매칭·callback 전달 전 관측 원칙을 참고하며 외부 코드·의존성은 추가하지 않는다. 실제 경계는 격리 설치한1.64.1의 `coreBundle.js`로 대조한다. 전역 Playwright나 브라우저 설치는 변경하지 않는다.

## E2E16 링크 재그리기와 E2E09 PTY 기록 ACK(t-1151)

- [원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40429411549/job/121943161404)의 로그와 artifact 목록·원본 자료를 확보했다. E2E16 JSON에는 설정의 go/popstate가 끝난 뒤 같은 좌표에 down/up이 도착하고, 그 뒤 파일 요청·pushState·viewer mount가 없는 순서가 남았다. 가상화 행의 중앙/측정 전에 클릭 자체가 빠진 경우다. 원본에는 Linkifier의 각 재그리기 호출이 기록되지 않아 당시 호출의 정확한 출처를 소급 확정하지 않는다.
- xterm 5.5.0은 viewport render에서 provider 응답을 재생성하면서 누름/놓음을 객체 동일성으로 판정한다. 실제 PTY가 누른 링크 행을 같은 대상·범위로 다시 그리게 하면 수정 전 실행본은 원본과 같은 viewer=false/preview=false/rows=0·20초 실패다. 이 대조의 JSON·PNG·trace를 보존했다. `xterm-5.5.0-link-press.patch`는 provider 순번·대상·전체 범위와 놓은 자리로 같은 링크를 판정하고 누름을 한 번 소비한다. 기존 앱의 왼쪽 단추·선택 없음 검사와 파일 링크의 현재 버퍼 재검증은 유지한다. 설정 복귀 대기는 그대로다.
- 기존 E2E16의 wrap 재열기에만 이 실제 PTY 재그리기를 넣었다. `clickLink`는 콜백 실패에도 눌린 마우스를 finally에서 놓는다. 클릭/요청/기록/선택·중앙·강조·줄 바꿈 계약과 휴대폰/iPad 조합·기존 상한을 유지한다. 진단은 활성 terminal 교체 뒤에도 실제 행 변경·링크 장식·포커스·누름 대상을 관측한다. 필요한 이유: 단순 정상 클릭만으로는 down/up 사이 render의 클릭 유실을 재현할 수 없다.
- E2E09는 원본과 로컬 첫 실행 모두 기대·실제가 같은 문자열인 상태에서 통지 대기가 만료됐다. `receivedIs`는 초기 읽기와 fs.watch callback에서만 비교하고 timeout의 최종 읽기는 실패 문구에만 쓴다. 따라서 파일의 최종 가시성을 마지막 유효 재검사로 보지 못한 상태다. 별도 관측판은 owner별 initial/notice/close의 원문 bytes·offset·시각을 보존했으나 그 실행에서는 누락을 재현하지 못했다. 원래 실패 당시의 kernel 통지 누락·미완성 UTF-8·watcher close 순서를 추측해 확정하지 않는다.
- `composition-pty.mjs`의 fixture는 실제 PTY stdin bytes를 파일에 동기 기록한 뒤 한 TCP 연결로 누적 byte ACK를 보낸다. 부모는 먼저 연결을 소유하고 ACK의 현재 상태도 검사해 대기 전 도착을 놓치지 않는다. 입력은 ACK된 Buffer 범위로 정확히 비교하며 UTF-8 중간 청크를 문자열로 바꾸지 않는다. 출력/flush/reset 제어도 같은 연결의 JSON 줄로 전달해 별도 control 파일 감시를 없앤다. echo 보관도 Buffer로 유지한다. 입력 bytes·조합 위치·확정 순서·중복 없음·실제 Chromium/native WebKit 및 WebKit replay 계약은 그대로이며 새 대기의 실패 상한은 5초다. 실패·종료에서 대기자·연결·listener를 정리하고 누적 ACK·파일 가시성·오류를 진단에 남긴다.
- 필요한 유닛은 세 경계다: 실제 upstream에 패치를 붙인 클릭 유닛(같은 대상 한 번·대상/범위/provider 변경·범위 밖·추가 놓음 거부), PTY ACK 유닛(대기 전 도착·UTF-8 중간 청크·합쳐진 ACK·다음 입력·byte 불일치·연결 종료), 기록보다 이른 ACK 거부/대기자 정리다. 모두 5초 미만이며 구현 모양을 확인하는 시험은 추가하지 않는다.
- 부분 검사는 웹 전체 유닛·타입/번들, 빌드 완료 뒤 고정한 e2e 실행본의 E2E09/16 전체와 E2E03 「그냥 끌어 고르기」, 공개 검사다. 수정 전 E2E16 대조는 실패하고 수정 후 같은 가상화 단계 단독은 4.19초에 통과했다. Chromium·WebKit·native WebKit의 조합 위치 단계도 4.35초에 통과했다. 캡처/trace를 켠 병렬 전체 결과와 일반 실행의 비용은 구별한다. 다른 기존 단계의 5초 감축은 별도 전체 시험 정리 범위다. 전체 ci-local·착지 검증은 coordinator의 착지 차례에 수행한다. 실패 재시도·고정 sleep·시한 확대·기대값 완화는 없다.
- OSS 후보는 [Node fs.watch 주의점](https://nodejs.org/api/fs.html#caveats)([MIT](https://github.com/nodejs/node/blob/main/LICENSE)), [libuv 파일 통지](https://docs.libuv.org/en/v1.x/fs_event.html)([MIT](https://github.com/libuv/libuv/blob/v1.x/LICENSE)), [xterm Linkifier](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Linkifier.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))다. 통지와 기록 완료를 구별하고 실제 provider/포인터 경계를 대조했다. Playwright 입력/사건 지침(Apache-2.0)도 참고했으며 외부 코드·제품 의존성은 가져오지 않는다.

## macOS pipe 창의 native exec와 실패 원문(t-1146)

- 기존 `pipe_window_cannot_leak_to_another_child_or_serialize_its_lifetime`의 시스템 shell 사본을
  Rust 시험과 같은 아키텍처로 빌드한 일반 exec launcher로 바꾼다. 재서명한 시스템 arm64e 실행 정책을
  FD 회귀에 섞지 않는다. launcher는 실제 로드한 cputype/cpusubtype·주입 여부·exec를 trace에 남기고,
  launcher/capture/holder는 DYLD 주입 누출을 거부한다. 판 조회가 덧붙인 `--version`은 이전 `sh -c`의
  위치 인자와 같이 fixture에 전달하지 않는다. 제품 생성 잠금·판 조회 코드는 바꾸지 않는다.
- raw의 FD 상속·자식 종료 뒤 EOF 대기와 나머지 모드의 상속 차단·holder 생존 중 반환,
  NOTE_EXIT·exit7·stdout/stderr 단언, 내부 2초·worker 5초 상한을 유지한다. 새 시험·E2E 번호는 없다.
  주입 제거를 빼는 변형에서 launcher exit91과 socket 연결 전 실패를 확인한다. 필요한 이유는
  SIP의 환경 삭제나 shell 아키텍처 때문에 누출 회귀가 숨거나 다른 원인으로 먼저 죽지 않게 하기 위해서다.
- timeout·읽기 오류에도 전체 worker stdout/stderr·trace를 먼저 기록하고, 실패한 실험실만 보존한다.
  macOS CI 실패 시 `rust/*-aaa_common.log`와 실험실의 trace/stdout/stderr를 artifact로 7일 보존한다.
  길이가 제한된 annotation을 전체 원문의 대체로 쓰지 않는다.
- 부분 확인은 `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib`와
  `RUST_TEST_THREADS=8`의 같은 명령, `cargo clippy --profile verify --locked -p aaa-common --all-targets -- -D warnings`,
  fmt·공개 검사다. 시스템 설정·SIP·boot args는 바꾸지 않는다. 전체 gate·착지는 coordinator 차례에 수행한다.
- 원래 main과 같은 macOS 15.7.9(24G830)의 [진단 run 40432899481](https://github.com/cLazyZombie/aaa/actions/runs/40432899481)에서
  시스템 shell·Apple 서명을 유지한 사본은 본문과 exec 뒤 exit7, ad-hoc 재서명 사본은 본문 진입 전 실패였다.
  arm64e slice 지정은 signal11, 기본 실행은 signal9로 원래 main의 signal4와 같지는 않았다.
  확정한 수리 경계는 재서명된 시스템 shell의 실행 실패이며, 정확한 trap 명령이나 OS 내부 거부 사유를
  확보한 것처럼 쓰지 않는다. native arm64 launcher는 exec·socket·NOTE_EXIT를 거쳐 네 모드를 0.29초에 통과했다.
- 참고 후보는 [XNU exec 정책](https://github.com/apple-oss-distributions/xnu/blob/xnu-11417.140.69/bsd/kern/kern_exec.c)
  (APSL-2.0), [Rust std spawn](https://github.com/rust-lang/rust/blob/main/library/std/src/sys/process/unix/unix.rs)
  (MIT/Apache-2.0), [Duct](https://github.com/oconnor663/duct.rs/blob/master/src/lib.rs)(MIT)다.
  실행 정책·생성과 출력 수집의 경계를 대조했고 외부 코드·의존성을 가져오지 않는다.

## E2E103 보관 본문과 복귀 조회 사건(t-1134)

- 보관 본문·과거 위치의 복원은 `ConversationRefresh`의 최신 읽기 lane과 독립적이다. 변경 알림으로 시작한 실제 `pane_chat` 응답을 `route.fetch` 뒤 보류하고 tasks 왕복을 수행하면, 본문은 82ms에 복원되지만 복귀 요청은 0회다. 이전 응답을 놓으면 복귀 요청이 정확히 1회 시작한다. 본문 준비 뒤 즉시 요청 수를 잘라 검사하던 기존 단언은 이 정상 순서에서 실패한다. 이 결정적 대조는 시험의 경합을 확정하며, 원본 t-1131 실패 당시의 개별 HTTP 순서를 소급 증명하지는 않는다.
- 기존 E2E103에서 복귀 전에 해당 pane의 최신 요청 사건을 구독하고, 본문 계측과 `Promise.all`로 함께 관측한다. 본문 준비·읽던 위치·동일 DOM·초안·과거 재요청 0회·복귀 최신 1회 계약을 유지한다. 응답 수는 요청 사건을 기다리기 전의 본문 완료 시점에 기록한다. 요청이 없는 경우 원래 사건 대기와 같은 20초에 실패한다. 고정 sleep·재시도·시한 확대·기대값 완화는 없다.
- Chromium 1280px·WebKit 390px의 coordinator 첫 시도에 실제 이전 응답 보류를 더한다. 응답을 놓기 전에 본문·과거 위치가 복원되고 복귀 요청은 아직 0회인지 검사한 뒤, 보류 해제 후 최신 요청이 정확히 1회인지 검사한다. 필요한 이유: 보관 화면의 즉시 복원과 직렬 읽기의 지연을 동시에 지켜 같은 거짓 실패가 돌아오지 않게 한다. 파일 감시는 내용 없는 빈 줄 추가로 일으키고 실제 host 응답을 사용한다. route는 보류 해제·전달 완료·해제 순서로 정리한다. 기존 조합과 시도 수는 유지한다.
- t-1127의 별도 `locator.evaluate` 30초 lookup/adoption 정지는 이 수리의 해결 범위로 세지 않는다. 원본의 DOM/Runtime 원문이 없으므로 정확한 정지 호출·원인은 미확정이다. 기존 수동적 protocol·함수 진입/반환 관측을 유지하며 원인 없이 locator를 우회하거나 시험을 지우지 않는다.
- 참고: [Playwright 요청 사건을 동작 전에 구독](https://playwright.dev/docs/events#waiting-for-event)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [React effect 정리와 늦은 응답](https://react.dev/reference/react/useEffect#fetching-data-with-effects)([MIT](https://github.com/facebook/react/blob/main/LICENSE)). 사건의 소유권과 이전 읽기 수명 원칙을 참고하며 외부 코드·의존성을 가져오지 않는다.

## 머신 끝 확인 시험의 원격 셸 격리(t-1142)

- `an_installed_machine_gets_a_check_pane_and_a_login_pane`는 실제 host·DB와 duplex 가짜 agent를 사용한다.
  host OS와 무관하게 POSIX·Windows 원격 머신의 argv·작업 폴더, 등록/연결/권한 거부와 로그인 pane을 검사한다.
  가짜 agent의 화면은 고정한 다섯 줄을 snapshot으로 주고 끝 표지를 다음 출력 청크로 보낸다.
  끝 표지 전에는 결과가 비어 있고, 뒤에는 다섯 줄이 정확히 저장되어야 한다.
  필요한 이유: 원격 머신 검사에 host의 `sh`·git·PATH를 실행하면 Windows의 깨진 셸 shim 출력이
  정상 화면처럼 들어가 실제 원인 대신 30초 뒤 「끝 확인 줄」로 실패한다.
- 실제 POSIX 셸 출력은 기존 Unix 전용 `the_check_script_reports_the_work_folder_git_harnesses_and_path`에서
  계속 검사하며 종료 코드와 끝 표지도 확인한다. 셸 실패에는 종료 코드·stderr를 즉시 남긴다.
  두 시험은 `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib machine_setup::tests::`로 확인한다.
  시험 삭제·제외·재시도·시한 확대·기대값 완화 없이 기존 host 요청 계약을 유지한다.
- 참고 후보는 [Rust Command의 출력·종료 상태](https://github.com/rust-lang/rust/blob/main/library/std/src/process.rs)
  ([MIT/Apache-2.0](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT))와
  [Tokio watch의 구독·변경 통지](https://docs.rs/tokio/latest/tokio/sync/watch/index.html)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))다.
  실행 성공과 출력 도착을 구별하고 현재 상태를 먼저 읽은 뒤 사건을 기다리는 원칙을 참고했다.
  외부 코드·의존성은 추가하지 않는다.

## macOS pipe 창의 worker 한정 주입(t-1137)

- main `c115d41a`·`3db8bcc7`의 macOS job 원문에서 최초 오류는 capture 스레드의 `status.code(): None != Some(7)`였다. raw holder 상속 확인과 pipe 해제는 성공했고 capture 역할은 시작하지 않았다. 뒤의 연결 Timeout을 시작 지연으로 취급하지 않는다. 원래 별도 `21-aaa_common.log`·`raw.trace` artifact는 없었고 job에 출력된 전체 aaa_common 구간과 trace를 추출했다.
- 같은 기준에 capture 반환의 status·stdout·stderr만 기록한 진단 `ca93fea6`을 사용자 허용 아래 일회용 branch에서 실행했다([run 40418222799](https://github.com/cLazyZombie/aaa/actions/runs/40418222799)). 실제 `/bin/sh`가 물려받은 `arm64` window.dylib를 `arm64e`에 넣지 못해 fixture 시작 전에 SIGABRT(6)로 종료했다. worker2485·holder2486의 상속/해제 뒤 capture2487의 dyld 오류와 worker exit101·양쪽 EOF가 남았다. 원래 두 실행의 capture PID·signal 번호·holder 종료 상태는 기록되지 않아 소급해 만들어 쓰지 않는다.
- `DYLD_INSERT_LIBRARIES`는 실제 pipe 창을 붙잡는 worker에만 넘기고 capture shell·holder에서는 제거한다. 제품 생성 잠금과 std/Tokio/실제 harness 판 조회의 동작은 그대로다. raw 상속·exit7·stdout/stderr·NOTE_EXIT·holder 생존 중 반환 단언과 내부2초·worker5초 상한도 유지한다.
- 기존 `pipe_window_cannot_leak_to_another_child_or_serialize_its_lifetime` 안에서 `/bin/sh`를 자기 target에 자식 프로세스로 복사하고 사본만 ad-hoc 서명한다. 보호된 시스템 shell이 DYLD를 지워 회귀를 숨기던 일반 Mac에서도 같은 loader 경계를 검사하기 위해 필요하다. 시스템 실행 파일·설정·SIP는 바꾸지 않으며 새 시험·E2E 번호는 없다. 주입 제거 전에는 같은 owner가 SIGABRT→Timeout으로2.11초에 실패했고 제거 뒤 네 모드가0.13초에 통과했다.
- 부분 검사는 `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib child_process::tests::pipe_window -- --nocapture`, 같은 owner의 `--test-threads=1`, `RUST_TEST_THREADS=8`의 aaa-common 전체와 `cargo clippy --profile verify --locked -p aaa-common --all-targets -- -D warnings`, fmt·공개 검사다. 최종 owner0.14초·병렬 전체59통과/보조1ignored1.04초와 나머지 부분 검사가 통과했다. 고정 sleep·실패 재시도·시한 확대·기대값 완화는 없다. 전체 ci-local과 착지는 coordinator 차례에서 수행한다.
- OSS는 [Rust std](https://github.com/rust-lang/rust/blob/main/library/std/src/sys/process/unix/unix.rs)(MIT OR Apache-2.0)의 spawn/output 경계, [Duct](https://github.com/oconnor663/duct.rs/blob/master/src/lib.rs)(MIT)의 생성 잠금/수집 분리, [dyld 환경 처리](https://github.com/apple-oss-distributions/dyld/blob/main/dyld/DyldProcessConfig.cpp)(APSL-2.0)의 보호 여부별 DYLD 처리와 [runner arm64e 주입 이슈](https://github.com/actions/runner-images/issues/9461)를 대조했다. 주입을 필요한 프로세스에 한정하는 원칙만 따르며 외부 코드·의존성은 가져오지 않는다.

## Windows host 저장 경계와 큰 fixture(t-1138)

- 원래 CI의 60초 경고 다섯 건 중 계정·업로드·sweep·brief는 뒤에 완료됐고 post가 단계 상한까지 남았다. 격리 Windows 재현은 시작/완료 시각·owner 단계 로그와 실제 자식의 native stack으로 구분한다. 업로드는 `SetSecurityInfo → private_file::protect`, sweep 준비와 post는 SQLite `FlushFileBuffers`, brief는 실제 thread 저장을 관측했다. 계정 첫 초기화에서는 실행 파일 hash 읽기도 관측했다. 이 결과를 원래 hosted runner의 대기 스택으로 대신 주장하지 않는다.
- Windows private 객체는 매번 실제 handle의 종류·소유권을 확인하고, protected 비트와 DACL 전체가 원하는 값과 같을 때만 보안 쓰기를 생략한다. `SetSecurityInfo`의 기존 자식 상속 재전파를 피하되 경로/ACL 캐시는 두지 않는다. 새 `windows_acl_tests::private_acl_recheck_avoids_security_writes_but_repairs_external_changes`는 실제 OS ChangeTime 불변·외부 ACL 변경 복구·private 접근을 검사한다. 필요한 이유는 같은 ACL 반복 적용의 비용과 잘못된 기존 권한을 그대로 통과시키는 보안 회귀를 함께 막기 위해서다. 수정 전에는 실제 ChangeTime 변경으로 실패한다.
- host의 CREATE/ALTER·schema 표지·지표 이행을 한 immediate transaction으로 확정한다. WAL·FULL synchronous는 유지한다. 새 `db::schema_tests::schema_upgrade_commits_once_and_rolls_back_on_failure`는 디스크 DB의 모르는 schema 거부·앞선 DDL rollback·정상 이행·재시작 저장과 내구 설정을 검사한다. 필요한 이유는 묶음 이행이 부분 schema나 열린 transaction을 남기지 않게 하기 위해서다.
- 기존 post owner의 30건 핑퐁·31번째 거부·19/20회 음소거·60초/시간/24시간 경계와 내용 중복을 유지한다. 시간/하루 창의 중간 이력만 본문 정리 뒤 원장 형태로 한 transaction에 준비하고, 200번째/2,000번째 접수와 초과 거부·경계 뒤 재접수는 실제 owner를 통과한다. 마지막 접수의 원장·inbox가 Host 재시작 뒤에도 남아야 한다.
- sweep의 601건 준비만 한 transaction으로 묶고 500/101/실제 cleanup·dry-run·재개 경합·저장 실패를 유지한다. 업로드는 999개까지 실제 디스크에 준비하고 다른 업로드로 cache를 비운 뒤 1,000번째 추가·초과 거부·상한에서 교체한 실제 바이트를 검사한다. 앞선 빈 파일/교체/실패 뒤 재계산·다른 업로드의 cache 경계도 유지한다. report fixture는 가짜 agent 취소 완료를 await하고 Host/DB가 닫힌 뒤 폴더를 지운다.
- 부분 확인은 `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib`, `-p aaa-common --lib`, 두 crate의 `clippy --all-targets -- -D warnings`다. Windows에서는 같은 명령을 격리 checkout 안에서 실행하고 프로세스 우선순위를 BelowNormal로 둔다. Cargo가 시험 실행 파일을 다시 만들 때 같은 실행 파일의 시험을 함께 돌리지 않는다. 실행 파일을 직접 쓰면 각 package 폴더를 cwd로 하고, build script가 쓰는 실제 Git checkout과 동작하는 Git/sh PATH를 준비한다. 기존 timeout·시험 선택·기대값·실패 재시도는 바꾸지 않는다. 전체 gate와 착지는 coordinator의 차례에서 한다.
- OSS 후보는 [SQLite transaction 비용](https://www.sqlite.org/faq.html#q18)([public domain](https://www.sqlite.org/copyright.html)), [Tokio task 완료](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)), [tempfile의 열린 handle과 명시 정리](https://docs.rs/tempfile/latest/tempfile/struct.TempDir.html)([MIT](https://github.com/Stebalien/tempfile/blob/master/LICENSE-MIT))를 비교했다. 묶음 commit·취소 완료·handle 종료 후 정리 원칙을 따르며 코드/의존성은 가져오지 않는다. ACL 재전파는 [Windows SetSecurityInfo 문서](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo)의 OS 계약과 실제 stack으로 확인한다.

## 큰 응답 회귀의 단계 계측과 평문 가림 비용(t-1141)

- 기존 `large_report_and_bad_parts_keep_requests_on_one_link`의 5초 상한 안에서 Host::open·manifest 준비·Welcome/pong·hash·큰 보고·분할 경계 가림·손상 조각·다음 요청·fake 종료·host reader 종료·파일 정리의 단계/전체 경과를 남긴다. 상한에 걸리면 마지막 단계와 전체 경과를 panic에 남겨 전송 비용과 종료 정지를 구별한다. 14,781개 manifest와 2MiB 초과 보고, 원본/hash 보존, 경계 가림, 오류 요청 격리와 같은 generation의 다음 입력 단언은 그대로다.
- 가림의 원문 창과 ANSI/CR/LF를 뺀 글의 창은 평문에서 같다. 평문은 원문 검사를 한 번만 하고 위치 배열과 두 번째 글 배열/검사를 만들지 않는다. ANSI/개행이나 이전 청크의 미완성 ESC가 있으면 기존 두 경로를 유지한다. 원문·겹친 8바이트 창·접두부 보류·끝 flush 의미를 바꾸지 않는다.
- 기존 `stream_keeps_no_secret_across_any_ansi_or_chunk_boundary_and_flushes_the_tail`에 평문·CR/LF를 더하고 CSI와 함께 모든 두 청크 경계를 검사한다. 전체 인증 문자열이 별표로 바뀌고 앞뒤 비밀 아닌 글과 ANSI가 보존되는 독립 기대값도 확인한다. 필요한 이유: 빠른 평문 경로와 ANSI/개행 경로를 오갈 때 접두부가 새거나 원문을 놓치는 회귀를 막기 위해서다. 시험 삭제·새 E2E 번호·고정 sleep·재시도·시한 확대·기대값 완화는 없다.
- 부분 확인은 `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib redact`, host의 `--lib large_report_and_bad_parts`와 `--lib`, 두 crate의 strict clippy다. Windows는 전용 시험 폴더에서 같은 owner와 host 전체를 실행하며 서비스·예약 작업·Run 키·사용자 PATH·방화벽 규칙의 전후 목록을 대조한다. 전체 ci-local과 착지는 coordinator가 준 차례에 실행한다.
- 같은 기본 병렬 host 전체 실행에서 대상은 macOS 3.166→1.241초, Windows 5.251초 상한 실패→2.230초 완료였다. 가림 단계는 각각 1.060→0.371초·4.708→1.499초다. Windows 수정 전은 가림을 마친 뒤 다음 손상 조각 대기에서 이미 지난 5초 상한이 보고됐다. 단독은 macOS 1.293→0.868초·Windows 0.873→0.719초다. 각 전후 한 번의 관측이며 원래 macOS CI 실패를 재현했다는 뜻은 아니다. Windows의 별도 `an_installed_machine_gets_a_check_pane_and_a_login_pane`는 수정 전부터 끝 확인 줄 대기에 실패하여 이 후보의 수리 범위와 분리한다.
- 참고 후보는 [Tokio duplex/역압](https://docs.rs/tokio/latest/tokio/io/fn.duplex.html)·[task 종료](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html)([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))와 [Hyper 연결 종료 정리](https://github.com/hyperium/hyper/blob/master/src/proto/h1/dispatch.rs)([MIT](https://github.com/hyperium/hyper/blob/master/LICENSE))다. 종료 사건과 소유권을 대조했고 외부 코드·의존성은 추가하지 않는다.
## 보고서 매체 진단·선택 로딩·압축(t-1135, §8.506)

- host `report_text_gzip_keeps_original_ranges_downloads_and_authorization`은 실제 host/SQLite/publish/router에서 큰 HTML의 여러 조각 gzip을 복원하고 탐색 bridge, Content-Encoding/Vary/길이, HEAD, gzip 거부, 원본 Range·내려받기·영상, 401/403/404/416을 확인한다. 필요한 이유: 압축이 권한·Range 또는 원본 bytes를 바꾸는 회귀를 막는다.
- `gzip_emits_before_upstream_finishes_and_keeps_read_failure`는 실제 channel의 첫 입력만 주고 종료 전 출력을 받는다. 다음 읽기 오류가 원인 그대로 전달되고 stream이 끝나야 한다. 필요한 이유: 전체 버퍼링으로 되돌아가거나 읽기 실패를 gzip 성공으로 숨기는 회귀를 막는다. 5초는 실패 상한이다.
- 웹 `reportMediaProbe`는 진단 대상을 같은 host의 보고 파일로 한정하고 표시 결과에서 cap/query/origin/원문 오류를 제외하는지, cookie 없는 HEAD 한 번인지를 확인한다. 필요한 이유: 진단이 비밀 주소를 노출하거나 매체를 재다운로드하지 않게 한다.
- 기존 E2E73에는 Chromium/WebKit×1280/390px의 실제 GET 보류→HEAD 200→401/명시 오류→다시 열기 502 실패·자동 재요청 없음·두 번째 사용자 누름의 현재 r 판 새 cap 재발급·원본 decode를 더한다. 만료 시각만 응답 fixture에서 과거로 만들며 host 시계·DB를 바꾸지 않는다. route는 사건으로 풀고 unrouteAll(wait) 뒤 context를 닫는다. 필요한 이유: 새 진단·만료 UI와 실제 복구를 유닛만으로 보증할 수 없다.
- 기존 E2E88은 metadata 선대기 대신 최초 src/source 부재를 검사한 뒤 실제 첫 클릭으로 재생한다. 기존 두 엔진×두 화면·질문 선택/주소·HTML sandbox·새 탭·직접 mp4/Range·보고 본문·답한 사본 보존 계약은 유지하고 조합별 step으로 구분한다. 필요한 이유: preload=none만 바꿔도 브라우저가 요청하는 회귀와 iOS 첫 누름의 source commit/play 경계를 막는다.
- 첫 E2E73의 Chromium 두 조합은 통과했지만 WebKit의 만료 안내가 없었다. 진단 실행의 `opens=[]/replies=[]`와 실제 ready DOM으로 응답 route가 호출되지 않은 것을 확인했다. WebKit의 service worker가 제어하는 요청은 page.route에서 빠질 수 있다는 Playwright 계약과 기존 E2E101 패턴에 따라, 응답을 제어하는 새 fixture의 context만 `serviceWorkers: block`으로 만든다. 제품 SW·권한·기존 E2E의 SW 계약은 바꾸지 않는다. 시한·기대값은 유지한다.
- 부분 확인: 웹 유닛·타입/번들, host `--lib gzip`·관련 library HTTP, 의존 경계와 E2E59/73/88, strict clippy·공개 검사다. 내부 검증은 nice 15, 자기 target, Playwright 1.64.1과 기존 내부 공용 브라우저만 쓴다. 전체 ci-local은 coordinator가 준 차례에서 수행한다. 고정 sleep·실패 재시도·시한/기대값 완화는 없다.

## 개별 시험의 5초 기준(t-1117)

- Rust 유닛·통합, Python unittest, 웹 유닛, 웹 E2E의 개별 case는 보통 부하의 단독 실행 또는 평소 병렬 실행에서 5초 미만으로 유지한다. 5초 이상인 case는 원인을 확인하고 수정하거나, 고유 계약을 지키는 더 가벼운 시험으로 대체한다. 부하가 몰릴 때만 넘는 case는 그 부하 근거와 함께 구분한다. 전체 파일 시간·여러 조합의 합계·빌드 시간은 개별 case 시간과 따로 기록한다.
- 전후 비교는 같은 main·profile·웹 번들·Playwright/브라우저·병렬도·계측 방식으로 한다. 시험을 바꾸므로 실행 파일의 hash 자체는 달라질 수 있다. 각 실행의 시작·끝 load(1·5·15분)와 CPU 수를 남기고, 긴 실행은 중간 load도 기록한다. 전체 gate 또는 다른 빌드와 겹친 결과를 보통 부하의 기준값으로 쓰지 않는다. 공유 검증 머신의 전체 gate와 계측 차례는 coordinator가 조율한다.
- 고정 시간 대기는 사건이나 가상 시계로 바꾼다. 대량 fixture는 실제 기능의 소유 경계를 검사하는 양을 남기고 반복 준비를 줄인다. 시계·저장·페이지·전송 경계를 다른 owner로 옮기면 원래 이름·이유·대체 위치를 표로 남긴다. 기존 고유 계약·실패 단언을 유지하며, 재시도·시한 연장·기대값 완화로 통과시키지 않는다.
- 계정 풀의 미관측·부분/오류 사용량 11입력은 같은 시험의 실제 Host/SQLite를 한 번 준비한다. 입력마다 이력 없는 새 계정을 써서 처음 선택·실제 Starting 원장·두 번째 선택·Resolved 뒤 선택을 모두 검사한다. 입력이나 단언을 나누거나 없애지 않고 반복 Host 생성만 줄인다. 확인된 사용 한도·인증 오류의 거부 owner는 그대로 둔다.
- Host DB 초기화의 CREATE·추가 열 SQL은 같은 transaction으로 확정한다. `db::schema_tests::schema_failure_rolls_back_create_and_alter_then_preserves_rows_on_reopen`은 중간 index·ALTER 실패가 기존 스키마·행을 바꾸지 않고, 정상 재열기가 보존되는지 검사한다. 필요한 이유는 반복 확정을 줄이면서 실패한 초기화가 반쪽 스키마를 남기는 것을 막기 위해서다. WAL·동기화 설정과 SQL literal은 바꾸지 않는다. 단독 시간 차이는 작아 평소 병렬의 효과는 별도 계측한다.
- E2E39의 독립 WebKit390/430×Safari/홈 화면 앱 context와 Chromium/WebKit helper 호출은 각각 이름을 붙인다. 같은 기기별 본문·전체 기록 순회·그림·오류 진단·정리를 유지하고, 준비도 별도로 측정한다. 기기 분리를 전체 시간 절감으로 계산하지 않는다. 한 문서 안의 연속 기록 순회는 나누지 않는다.
- E2E39의 본문 목적지 순회와 별도 페이지의 할 일 상세 기록은 각각 이름을 붙인다. 설정 내부 복귀도 원래 별도 context였던 작업 패널·닫기 버튼을 각각 측정한다. 같은 여덟 context의 호출 순서·원래 움직임·기록 순회·오류 진단·정리를 유지한다. 부분 실행의 상세 기록은 같은 실제 thread 자료를 한 번 준비한다. 이 분리는 총시간 절감으로 계산하지 않으며 실제 브라우저·전후 시간은 별도로 확인한다.
- m13의 `park_returns_the_slot_and_unpark_resumes_the_same_conversation`은 실제 tick 뒤 보류·같은 대화·자리 반환·재개를 유지한다. 2.1초 고정 대기가 맡던 지난 기한의 보류 제외는 기존 `transitions::tests::auto_resolve_counts_from_the_latest_change_or_host_start`가 실제 pass의 자격 판정과 시각 주입으로 검사한다. 이 owner는 열린 쉼의 기한 직전/정각, host 시작·최근 보고 기준, 보류·평가자·작업/답변/검토 대기를 함께 지킨다.
- m13의 `rollout_*` 세 keeper는 바로 실제 사본 host/agent를 띄운다. 곧 버리던 원본 host/agent 기동과 쓰지 않는 Git 저장소 준비만 생략한다. 같은 실제 TLS 등록·복사 이미지·전송 proxy·설치/복구·서비스 호출·진행 중 작업/허가/실행 신원/원장 단언을 유지한다. 일반 World는 기존 Git 준비와 기동 순서를 유지한다. 이 두 소스 수리의 성능과 실제 실행은 별도 검증한다.
- E2E113·114의 확인창 클릭은 원래 animation.finished 뒤 native click한다. 공통 `inList`도 새 drawer의 원래 transition 종료를 기다린다. 움직임을 끄거나 끝내지 않으며 기존 클릭30초 예산을 대기와 함께 쓴다. 기존500/601건·실패/명시적 재개·응답 보류 중 반응성·원문/원장·모든 단언은 유지한다.
- 짧은 CLI `front_call`은 한 `block_on` 안에서 writer·Front 응답을 끝내므로 current-thread runtime을 쓴다. 기존 handshake·권한·구판 거부·timer·반환 계약은 같고 host/agent runtime 구성은 그대로다. 모의 Front 사이에 사건을 넣은 실제 CLI와 기존 실제 host/TLS/PTY owner로 확인한다. 병렬 wall 효과는 빌드를 분리한 같은 조건의 전후 계측으로 판단한다.
- 기억 회수의 첫 비공개 scratch는 검증한 파일을 직접 쓴다. 이 폴더는 병합 뒤 통째로 버리므로 파일마다 추가 임시 이름과 rename을 만들지 않는다. 크기/hash/경로·최신 host 원문·충돌/상한 검증과 최종 `memory::write`·원자 교체/복구는 유지한다. 기존 `memory_return_preserves_edits_made_on_host_while_thread_is_open`의 두 프로토콜·재열기·원문 보존·1000개 병합 거부와 `previous_agent_keeps_input_and_memory_transfer_with_legacy_file_frames`를 확인한다. 시험과 단언은 바꾸지 않는다.
- m13 `World::add_remote`는 실제 TLS 등록 뒤 agent를 띄우기 전에 Front 상태를 구독하고 `Connected` 사건으로 완료한다. CLI 재조회로 연결을 기다리던 준비 비용만 줄인다. 실제 등록·기동·같은 연결 상태 판정·30초 실패 상한은 유지하고 실패 시 host/agent 로그를 보존한다.
- 엔진·폭 등 독립 조합의 합계였던 step은 각각 이름을 붙여 개별 시간을 드러낼 수 있다. 이 분리는 전체 실행 시간의 절감으로 계산하지 않는다. 한 업무 흐름을 숫자만 작아지도록 잘라 완료 기준을 흐리지 않는다. 준비·정리는 별도로 측정하고 전체 비용에도 포함한다.

- t-1117의 시간 경계 owner는 다음과 같다. 새 시험은 벽시계 대기를 없애면서 기존 실패 경계를 유지하기 위해 필요하다.

| 실제 연결에서 유지하는 계약 | 시간·크기 경계 owner와 필요한 이유 |
| --- | --- |
| 준비/정리의 실제 Git gate·완료 전 상태·완료 결과 | `link::supervision_tests::preparation_and_cleanup_keep_their_deadlines_past_the_general_request_limit`: 일반 15초 초과와 설정 시한의 직전/정각·pending 정리를 가상 시계로 지킨다. |
| 무반응 PTY의 한 번 전송·수동 재전송·brief 전 메시지 거부 | `harness::tests::unknown_brief_keeps_the_accept_deadline_and_never_retypes_without_a_person`: 실제 Host/SQLite/AgentConn에서 기본 받음 시한 10초와 자동 재전송 금지를 가상 시계로 지킨다. m13 keeper는 실패 fixture의 `accept_wait_ms=100`을 명시한다. |
| nudge의 실제 CLI/PTY·draft 보호·대상/중복/권한 | `inbox::tests::nudge_windows_reset_on_changed_observations_and_require_two_empty_checks`와 `nudge_hook_idle_and_started_wait_for_the_exact_configured_window`: 화면·대상 reset, 두 관측, idle/quiet 직전과 정각을 시각 주입으로 지킨다. |
| CI/Git의 실제 조회·ETag·재시작·복구·중복 방지 | `watch::timing_tests::poll_waits_from_completion_and_never_overlaps_an_inflight_source`: 완료 뒤 간격과 조회 겹침 방지를 시각 주입으로 지킨다. fake 외부 명령 실험실만 `AAA_TEST_ROUTINE_SECONDS=1`과 `AAA_TEST_WATCH_POLL_MS=10`을 함께 준다. 논리 catch-up 주기와 운영 기본값은 유지한다. |
| routine의 승인·로컬/원격 실제 명령·출력/변환 보존 | 기존 `routine::tests::daily_due_in_local_time`의 직전/정각·일별/시간대와 실제 keeper를 나눈다. keeper의 `every 2s`는 유지하고 기존 `AAA_TEST_AUTOMATION_CLOCK_FILE`의 판정 시각만 원자 교체한 뒤 실제 tick과 각 routine의 `last_run_ms` 확정을 기다린다. tick만 끝나면 dispatch 잠금 때문에 일정 처리를 건너뛰었을 수 있다. 일정 cursor도 같은 시계를 쓴다. |
| 실제 CPU·메모리 표본의 agent→host→웹 표시 | `health::tests::stats_send_at_fifteen_seconds_without_catchup_or_replacing_the_last_check`: 실제 agent/cache/LinkMsg로 기본 15초 직전/정각·잠든 뒤 한 번·점검 결과 보존을 가상 시계로 지킨다. E2E27만 제품 설정 `health.every_s=1`, 실제 CPU 최소 표본 구간 1초·원래 표시/범위/host 값 단언을 유지한다. |
| 압축의 실제 Worker·raw ACK·history 정체 중 실제 PTY 입력 | 기존 `rateLimitedWire.test.mjs`의 수동 시계로 1Gbit/1ms·5Mbit/50ms·1Mbit/100ms의 FIFO·반대 방향 독립을 지킨다. E2E97은 실제 세 corpus×두 codec의 최종 xterm ACK와 history 전달 gate를 유지한다. |

- 조합 분리는 E2E06·98·102·108·113·114·115처럼 새 context/독립 fixture였던 경계에만 한다. E2E108의 모든 9개 원본·3개 탭·두 엔진×3개 폭·Reader 권한과 E2E98의 실제 수집·구판/끊김·뒤로/앞으로 단언을 유지한다. `--list`와 `--grep`으로 각 case를 선택할 수 있다. E2E12는 t-1123 수리가 착지한 뒤 그 결과 위에서 다룬다.
- d-0385의 E2E76 정렬 비용은 네 독립 Chromium/WebKit×desktop/iphone case로 측정한다. 각 case는 기존 5속성·320px 포함 배치·두 reload·나중 원문/활성 정렬 분리·실제 host 예약/원문 불변을 유지한다. t-1126의 마지막 시트 popstate helper는 그대로 쓴다. 휴대폰 시트는 원래 애니메이션의 `finished` 사건 뒤 정상 클릭하여 Playwright의 안정성 확인 간격 비용을 피한다. 애니메이션 속도나 클릭 검사를 바꾸지 않는다. 공통 준비와 파일 합계도 따로 포함하며 조합 분리를 합계 절감으로 계산하지 않는다.
- 재사용 WebKit은 76회차의 `specCleanup.own`으로 비동기 생성부터 소유한다. `e2eCleanupScope.test.mjs` 두 회귀는 정리 도중 늦게 생성된 자식의 유실/이중 종료와 다른 정리·생성 오류가 사라지는 일을 막기 위해 필요하다. 실제 WebKit 자식·실패 종료는 착지된 E2E12 수명 keeper가 유지한다.
- `lib/menu.mjs::clickMenuItem`은 시트의 원래 `animation.finished` 뒤 정상 클릭한다. 데스크톱 popover도 같은 항목을 정상 클릭하며, 움직임 대기와 클릭은 기존 30초 클릭 예산을 함께 쓴다. E2E56·76·85·86·108의 기존 메뉴 동작·원문/권한·시트 퇴장 단언은 유지한다. 애니메이션 도중 입력하는 keeper에는 일괄 적용하지 않는다.
- E2E55 초기 검사 keeper에는 실제 사이드바 갱신을 이름 `input` capture와 bubble 사이에 끼운다. 이름 칸 정체성·입력값·두 번 열기/저장과 초기 검사 중 편집 금지 단언을 모두 유지한다. 필요한 이유: 초기 로딩이 끝난 같은 DOM에서도 부모 commit이 controlled 값을 복원해 이름을 지우는 회귀를 잡아야 하기 때문이다. 새 시험을 늘리거나 입력을 다시 채우지 않는다. 제품은 대화·직접 답·계정 별칭과 같은 window capture에서 원래 trim 규칙으로 이름을 받으며 기존 onChange도 유지한다.
- m13 `RESUMABLE_AGENT`는 hook 없이 실제 화면 감지로 입력 받음을 보인다. Enter 뒤 작업 중 표지를 그린 채 자신의 실제 pane 상태가 working일 때까지 관측하고, 그 뒤 쉼으로 돌아간다. 시작 유예 0에서 먼저 감지된 가짜 Claude가 아무 받음 사건도 내지 않아 기본 10초를 기다리는 비용을 피한다. 관측용 CLI에는 같은 실험실 host의 상태 경로를 명시한다. 논리상 원격 labB의 로컬 상태로 호출하면 host socket이 없어 harness 자체가 종료되며, 실제 종료를 즉시 알리는 경로에서 brief 전달 실패로 드러났다. 실패 시 이 CLI의 stdout·stderr도 남긴다. 기본 받음 시한·원래 대화 ID/인증/보류/재개 단언·위조 hook 거부는 그대로다.
- host가 화면으로 먼저 신뢰 질문을 만들고 답한 뒤 agent의 첫 감지가 올 수 있다. `detect::watch::tests::an_input_before_the_first_observation_keeps_the_original_prompt_refresh_boundary`는 감지 요청 전/후 입력 두 순서에서 초기 `Unknown`도 기존 800ms 보호를 받는지 검사한다. 799ms에는 재질문하지 않고, 실제 창이 남으면 입력부터 잰 800ms에 다시 묻는다. 다음 실제 질문도 허용하며 원래 m13 질문·메시지 단언을 유지한다. 새 시험이 필요한 이유는 답한 신뢰 창이 첫 감지 결과로 다시 열리는 경합을 시간 대기 없이 재현하기 위해서다.
- E2E108 휴대폰 필터는 최초 메뉴 열기·Escape 초점 복귀·첫 실제 포인터 선택과 퇴장을 원래 애니메이션으로 검사한다. 뒤의 rc/aaa/정하지 않음/전체 네 반복만 실제 메뉴 단추의 Enter 활성화를 사용한다. 이때 기존 `focusIs`의 실제 focusin 사건으로 메뉴 첫 항목의 초기 초점을 받은 뒤 대상 단추에 Enter를 보낸다. 첫 rAF 초점이 대상 focus와 Enter 사이에 끼면 전체 필터가 잘못 선택되는 경합을 실제 메뉴의 초기 focus 호출 보류로 재현했고, 같은 사건 순서에서 수리를 확인했다. 진단 장치는 정규 CI에 넣지 않는다. 기본 애니메이션·실제 퇴장·14개 단언과 4개 DOM 검사를 유지하며, 움직임 축소나 force 클릭을 적용하지 않는다. 반복 포인터 경로의 owner는 같은 case의 첫 실제 선택과 E2E76의 네 조합이다.
- E108의 실제 「모두 읽음」 확인은 `clickDialogButton`으로 원래 확인창의 `animation.finished`를 받은 뒤 정상 클릭한다. 휴대폰의 시트 애니메이션과 실제 확인·읽음 범위 원장 단언은 그대로이며, 메뉴와 확인창이 기존 클릭의 30초 예산을 움직임 대기와 함께 쓴다.
- m13 `World::wait_show`는 먼저 Thread 사건을 구독하고 실제 CLI의 첫 사본을 확인한 뒤 같은 조건의 사건을 기다린다. `wait_screen`은 같은 visible 사본·substring·원래 시한의 실제 `pane wait-output`으로 끝낸다. 관측마다 100ms 간격으로 CLI를 다시 띄우던 비용을 줄이며 실제 동작 CLI·PTY와 결과 단언은 유지한다.
- `chat_paths_tests`의 rollout fixture는 원래 metadata와 301개 메시지를 그대로 쓰되 BufWriter를 명시적으로 flush한 뒤 파일을 읽는다. `coordinator_user_quota_never_auto_mutes_and_survives_standard_mute`는 최초 사용자 알림 30건·중복·한도 초과·일반 한도의 실제 최초/30번째와 20번째 거부/mute·음소거 뒤 사용자 알림·시간당60번째/초과/다음시간 허용을 유지한다. 두 창의 중간 56건만 같은 quota 원장에 transaction으로 준비한다. 실제 전달·원자 접수·재시작은 기존 `post_rate_duplicate_mute_and_restart`가 소유한다.
- 계정 신뢰의 정책×harness×머신×작업 형태 16조합과 첫 실행의 설정×머신 10조합은 각각 `accounts::account_trust_*`·`accounts::account_onboarding_*`라는 독립 case다. 각기 새 실험실에서 원래 모든 입력·13개 단언 본문을 유지하고, 로컬 case에서는 쓰지 않는 원격 agent만 준비하지 않는다. 나누기 전 합산61/34초를 개별 속도 절감으로 계산하지 않는다. 새 기능 시험을 늘린 것이 아니라 원래 독립 조합의 시간·실패·단일 선택 경계를 드러낸다.
- `big_inbox_lists_within_frame`의 원래 4,002개 파일·실제 inventory/import CLI·2+4,000 count·본문·차례·frame 한도·완료 뒤 최신순 단언은 유지한다. migration의 inbox UPSERT를 기존 SQL/JSON으로 한 transaction에서 실행하며 동기화 설정을 바꾸지 않는다. 새 `db::inbox_tests::import_batch_commits_together_and_rolls_back_updated_rows_on_failure`는 뒤 행의 실제 SQL 오류가 앞 행의 JSON과 done/done_at 열 갱신까지 rollback하고 정상 묶음은 함께 commit되는 경계를 지킨다. 필요한 이유: 대량 저장 실패 뒤 일부 항목만 옮겨진 상태로 남겨서는 안 된다. 기존 단일 `inbox_put`의 저장 의미는 그대로다. [rusqlite 0.37의 transaction 계약](https://docs.rs/rusqlite/0.37.0/rusqlite/struct.Transaction.html)을 따른다.
- agent는 session host의 종료 `Updated`를 받으면 harness의 `exited`도 바로 알리고 그 감시를 멈춘다. 늦은 연결을 위한 session host의 기본 2초 응답 유예·마지막 화면·진행 중인 요청 pump는 유지한다. 새 `harness_exit_is_published_while_the_session_host_still_answers`는 실제 PTY 종료와 8초 유예를 사용해, 종료 사건 시점에도 같은 session host PID가 살아 있고 마지막 화면을 읽는지 검사한다. 필요한 이유: 이미 끝난 harness의 상태를 session host 프로세스 종료까지 늦추는 경합을 막는다. 원본은 유예 종료 뒤 PID 단언으로 실패했고 수리판은1.15초에 통과했다. 기존 `closing_ended_sessions_does_not_wait_for_the_session_host_linger`의 닫기 owner도 유지한다. 시한을 늘리거나 유예를 짧게 하여 통과시키지 않는다.
- E2E28은 메뉴의 세 기기·진행 목록의 desktop/두 Reader/완료 보존·모델의 실제 기록/기기·테마/재시작을 같은 순서의 이름 있는 단계로 기록한다. 준비·정리 비용도 남기고 원래 assert·throw·domIs 76개와 모든 조합을 유지한다. 요약 없는 준비 thread는 기존 사람 대기 fixture를 써서 무관한 완료 droplet이 메뉴를 가리지 않게 한다(완료 알림·만료 owner는 E19/E17). `/model`은 실제 PTY에 보내고 모의 harness의 수신 출력을 확인하며, 새 기록을 쓰기 전 기존 모델이 유지되는지도 검사한다. 초기 메시지·Follow-ups 계약은 같은 파일 첫 단계와 m13에 남는다. 공유 nowClock은 시계 설치 뒤 명시적 reload로 UI의 첫 timer부터 제어한다. 부분 실행도 원래 경과 시간 단언을 지키며 실패를 벽시계 tick이나 재시도에 맡기지 않는다. 단독 전후 전체 44.597→29.812초이고 최종 시작·준비·정리를 포함한 29단계는 최대4.304초다. 평소 병렬 시간은 다음 전체 차례에서 따로 확인한다.
- 기본 post 한도 keeper는 시간당 최초·30번째·60초 직전 거부/정각 허용·200번째·1시간 직전 거부/정각 허용을 실제 요청으로 검사한다. 중간 196건은 같은 원장에 두 transaction으로 준비하여 앞 경계 판정에 미래 이력이 섞이지 않게 한다. 하루 최초·2,000번째·24시간 중복 우선/한도 경계도 실제 요청으로 유지하고 중간 1,998건만 준비한다. 30건 핑퐁·20번째 거부의 mute·기존 재시작/중복/원자 저장 owner는 그대로다.
- 변경 후 전체 시간과 5초 이상 목록은 배정된 보통 부하 차례에서 다시 수집한다. 가벼운 개별 통과나 AST 단언 대조·syntax 검사만으로 전체 통과·절감률을 주장하지 않는다. 새 main을 통합하면 비교에 영향을 주는 변경도 구분한다.

## 큰 agent 응답과 보고 hash 확인(t-1131, §8.505)

- 공통 `response_parts`의 사고 회귀는 원본 9,820바이트·빈 파일35개·건너뛴 파일14,746개를 합쳐 운영 사고와 정확히 같은 2,232,861바이트 frame을 만든다. 실제 duplex/writer/FrameRx에서 6조각의 원문 동일성과 구판의 `too_large`, 잘못된 큰 제어 frame 거부 뒤 같은 연결의 pong·다음 요청을 검사한다. 필요한 이유: 한 큰 응답이 모든 agent 요청과 재연결을 끊던 회귀를 실제 포장/전송 경계에서 막는다.
- 같은 공통 모듈의 손상 회귀는 전체/조각/연결 합계 상한, 취소 정리, 시작 없는 offset·중복 시작·순서/ID/종류 불일치를 검사한다. 필요한 이유: 새 분할 경로가 다른 요청에 결과를 주거나 미완성 자료를 무한히 쌓지 못하게 한다.
- agent의 `report_hash_probe_never_walks_the_library`와 host의 `current_report_hash_uses_the_negotiated_probe_and_legacy_fallback`는 실제 보고 파일의 hash·부재 오류와 기능 유무별 실제 요청 종류를 검사한다. 필요한 이유: 변경 확인이 library 전체 목록을 만드는 원래 경로로 돌아가지 않게 한다.
- agent의 `large_report_handler_negotiates_parts_and_keeps_legacy_links`는 실제 Agent/Welcome/요청 handler와 2MiB 보고 파일을 사용한다. 협상한 host의 분할 원문·hash와 구판의 명시 오류, 오류 뒤 hash·ping이 같은 연결에서 오는지 검사한다. 필요한 이유: 공통 codec 시험이 대신할 수 없는 Welcome 기능 설정과 실제 응답 진입점을 지킨다.
- host의 `large_report_and_bad_parts_keep_requests_on_one_link`는 실제 Host/SQLite/link reader와 14,781개 manifest를 사용한다. 원본 report 전송과 분할 경계를 가로지르는 가짜 인증 문자열의 CI 가림, 잘못된 조각의 요청 오류 뒤 같은 generation의 다음 입력을 검사한다. 필요한 이유: 재조립이 원본 hash 확인과 저장 가림의 순서를 바꾸거나 agent 연결을 닫지 못하게 한다.
- 부분 실행은 `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib response_parts`, agent의 `--lib report_hash_probe`·`--lib large_report_handler`, host의 `--lib current_report_hash`·`--lib large_report_and_bad_parts`다. 고정 sleep·재시도·시한 확대·기대값 완화는 없다. 새 E2E 번호는 추가하지 않으며 전체 gate는 coordinator에게 준비됨을 요청한 뒤 받은 차례에 실행한다. 운영 상태·설정은 읽기만 하며 재시작·설치·배포하지 않는다.
- 첫 전체 gate의 m13 `report_state_precedes_completion_event`는 보고 사건 전에 `routine_toggled`를 발견해 실패했다. 보존 DB에는 최초3건 뒤 fixture의 빈 파일 읽기로 생긴 `routine_broken(seq4, +++ 줄로 시작해야 한다)`와 `routine_toggled(seq5)`가 있었다. 감시 폴더 밖에서 완성한 fixture를 같은 파일시스템의 rename으로 공개한다. 상태/사건 단언·시한·기대값은 유지한다. E2E103의 복귀 최신 확인0회는 기존 main 간헐 정지의 별도 조사 범위이며 이 후보에서 시험·웹을 바꾸지 않는다.

## E2E76 마지막 시트 복귀·E2E81 실제 클릭과 최신 조회(t-1126)

- main e3bcc847의 E2E76 `closeBeforeReload`는 이미 닫기 전에 popstate를 듣지만 앞서 닫은 ⋯ 메뉴의 늦은 사건도 완료로 받았다. 실제 Chromium 390px trace에서 메뉴 popstate 때 상세 DOM이 남았고, helper가 돌아온 뒤에도 상세의 `aaa:sheet` 기록이 남은 채 첫 reload가 시작됐다. 이번 자연 관측에서는 reload 자체는 성공했다. 원래 CI의 두 reload 중 어느 것이 실패했는지는 잘린 입력에 없어 확정하지 않는다.
- 격리한 실제 host·브라우저에서 메뉴 back과 상세 back의 전달만 사건으로 제어했다. 수정 전 순서는 `back1 → pop1 → back2 → helper 반환 → pop2`로 실패했고, 수정 후 `back1 → pop1 → back2 → pop2 → helper 반환`으로 468ms에 통과했다. helper는 상세 DOM과 마지막 시트 기록이 모두 없는 popstate까지만 완료로 받는다. 제품 history·정렬 저장·요청은 바꾸지 않는다. 기존 E2E76의 두 reload, 양 엔진 ×1280/390px와 320px, 예약 차례·TASKS/나중 원문 불변·기기 기억·이동 도구 계약을 유지한다.
- E2E81은 두 제품 경합을 별도로 재현했다. 실제 사용량 다시 읽기 클릭을 root capture/bubble 사이에 끼우면 기존 controlled checkbox의 클릭 의도가 유실되어 `locator.check: Clicking the checkbox did not change its state`가 난다. 저장 뒤 조회2를 실제 accounts 사건의 조회3이 앞지르게 하면, 조회2가 버려졌는데 busy/pending이 먼저 풀려 host에서는 해제한 스위치가 화면에서는 checked=true/disabled=false로 돌아왔다.
- `AccountAutoAssign`은 click capture에서 실제 선택을 먼저 보존하고 기존 onChange도 유지한다. `useAccountPool`의 동기 잠금이 같은 클릭의 중복 저장을 막는다. 저장 뒤 읽기가 새 읽기에 밀리면 이미 시작한 최신 Promise가 끝날 때까지 기다린다. 요청을 재시도하거나 추가 조회로 성공을 만들지 않는다. host 설정·revision과 최신 읽기만 정본이며 다음 조작은 그 결과와 함께 활성화한다.
- 기존 E2E81의 같은 네 화면에서 실제 해제→재사용, capture/bubble 사이 부모 갱신과 각각 정확히 한 번의 저장을 검사한다. Chromium 1280px에서 실제 mutation 응답과 세 조회의 전달을 제어해 최신 조회 전 busy/선택 유지·옛 응답 무시를 확인한다. WebKit 390px에서는 실제 요청을 보내기 직전에 다른 CLI 변경을 확정해 revision 충돌을 만들고 원래 사용 상태 복귀·오류 표시·진행 중 계정 불변을 확인한다. native click/input/change·DOM checked/disabled와 비밀 없는 pool 요청/revision/응답은 실패 시 `diagnostics/account-switch-<engine>-<width>.json`에 보존한다. 이 회귀는 실제 클릭 유실과 저장 미완료 상태의 다음 입력을 각각 막기 위해 필요하며 새 실험실·E2E 번호는 늘리지 않는다.
- 확인: Playwright 1.64.1·내부 공용 브라우저·nice 15·worker 1의 E2E76/81 전체가 각각25.19/13.06초, wall38.25초에 통과했다. 개별 계정 스위치 단계는3.96초, E2E76 정렬 단계는19.96초다. 후자의 기존5초 초과 비용은 별도 d-0385/t-1117 경량화 범위에 남긴다. 웹606유닛·타입 검사·번들도 통과했다. 원래20초 사건/30초 locator·reload/120초 단계 상한과 기존 단언은 유지하며 고정 sleep·중복 클릭·입력값 직접 변경·실패 재시도·시한 확대는 없다. 전체 ci-local·착지는 coordinator의 순서를 기다린다.
- OSS 후보는 [Playwright 탐색](https://playwright.dev/docs/navigations#waiting-for-navigation)·[checkbox 상태 검증](https://playwright.dev/docs/api/class-locator#locator-check)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [React effect의 응답 순서와 정리](https://react.dev/reference/react/useEffect#fetching-data-with-effects)([MIT](https://github.com/facebook/react/blob/main/LICENSE))다. 사건 소유권·최신 응답·실제 입력 원칙을 참고했으며 외부 코드·의존성은 가져오지 않는다.

## E2E83 다시 열기의 웹 교체 완료와 실패 처리(t-1100)

- 첫 전체 gate의 유일한 실패는 E2E83의 모든 thread 메뉴 이동이었다. `hashIs(#threads)`는 20초 뒤 새 `#pane=…`에 남았고, 먼저 시작한 대기 Promise가 click 대기보다 먼저 거절되어 Node unhandled rejection으로 child를 끝냈다. 알려진 m13 계정의 labA 연결 실패는 이 실행에서는 없었다.
- 기준 main `1f58f884`와 후보의 App/원래 E2E83/harness는 같았다. 실제 host/agent에서 새 pane이 running인 뒤 replacement WS state만 메뉴를 연 직후까지 보류했다가 전달하면 둘 모두 옛 pane의 열린 메뉴→새 pane으로 교체/메뉴 소실이 나왔다. 첫 단계가 host 기록만 보고 끝난 것이 다음 단계와 경쟁한 원인이다. 제품의 정상 pane 교체를 바꾸거나 부하 탓으로 단정하지 않는다.
- 기존 E2E83 한 실험실에서 reload 뒤 실제 state를 가로채 같은 지연을 만든다. host의 새 pane running과 state 수신 뒤에도 화면이 옛 pane임을 단언하고, state를 놓은 뒤 새 주소와 실제 `.term` 교체까지 사건으로 기다려 첫 단계를 끝낸다. 주소 교체는 replaceState이므로 hashchange만 기다리지 않는다. 모든 thread 메뉴 이동의 hash/click Promise는 생성과 동시에 Promise.all로 묶어 실패가 단계 진단/정리를 건너뛰지 않게 한다.
- 필요한 이유는 host 성공과 웹 적용 완료의 차이를 결정적으로 만들고, 같은 메뉴 소실이 다음 단계에 새지 않도록 하기 위해서다. 원래 20초/locator/단계 상한과 나머지 CLI 0회 계약을 유지한다. 고정 sleep·실패 재시도·시한 확대·시험 제외는 없다. 기준 main과 후보 실행 파일에 같은 수정 owner를 적용해 E2E83 단독을 확인하고 커밋한 SHA로 다음 전체 gate를 수행한다.
- 기존 E2E109의 실제 WS gate와 [Playwright WebSocket routing](https://playwright.dev/docs/network#websocket-mocking)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [WPT WebSocket event 검사](https://github.com/web-platform-tests/wpt/tree/master/websockets)([BSD-3-Clause](https://github.com/web-platform-tests/wpt/blob/master/LICENSE.md))의 사건 순서 원칙을 참고한다. 외부 코드를 가져오지 않는다.

## 계정 이름 변경과 재로그인 없는 시작(t-1100)

- `cargo test --profile verify --locked -p aaa-host --lib account_rename` 세 저장 회귀는 실제 암호화 금고/SQLite/프로젝트 설정으로 인증·판·세대/metadata/기본/풀/대기 시작/보류 thread/binding 유지, agent·Reader·중복·준비 중 실행 거부, DB 실패 뒤 원문 복구와 양쪽 확정점의 Host::open 복구를 검사한다. 필요한 이유는 인증 AAD 변경·참조 누락·부분 이전이 계정 사용을 끊거나 다른 신원으로 시작하게 하는 회귀를 막기 위해서다.
- `cargo test --profile verify --locked -p aaa-host --lib rename_during_quota_collection`은 수집기가 cache를 잡은 경계에서 이전을 끝내고, 옛 cache가 남아도 새 이름의 저장 관측·남은 양을 유지하고 옛 키를 재저장하지 않는지 검사한다. 필요한 이유는 웹 자동 사용량 조회가 이름 변경을 막거나 관측을 잃는 경합을 막기 위해서다.
- 기존 m13 accounts에 `renamed_account_starts_with_the_same_auth_without_login_and_rejects_while_running`을 더한다. 가짜 인증을 import→기본 지정→CLI rename한 뒤 실제 host→agent→session→PTY 자식이 같은 인증 hash를 받아야 한다. 실행 중 재변경은 거부하고 그 자식은 같은 인증으로 끝난다. 필요한 이유는 금고 단위 시험이 실제 시작 전달을 대신할 수 없기 때문이다.
- 기존 E2E58에 Chromium/WebKit × 1280/390px의 행별 폼·잘못된 이름/중복·저장 직전 다른 기기의 중복 생성·서버 거부 후 원본 유지·성공 결과·기본/메일/인증일/판 유지·CLI 역변경을 더한다. Reader는 단추가 없고 직접 변경 요청도 거부한다. 필요한 이유는 실제 웹 권한·응답·휴대폰 레이아웃을 함께 지키기 위해서다. 사건/응답/DOM으로 끝내고 고정 sleep·실패 재시도·시한 확대는 없다.
- 이전 gate의 중첩 read·writer 깨움 시험 하나는 시작 경로 중첩 중 이름 변경이 교착하지 않는 계약을 지킨다.
- 부분 확인은 위 owner와 E2E58·65·81, proto TypeScript·의존 경계·웹 타입/유닛·strict clippy·공개 검사다. 전체 ci-local은 coordinator의 실행 차례를 받은 뒤 수행한다. Playwright npm 1.64.1은 작업 폴더에 두고 내부 공용 브라우저만 사용한다. 운영 DB/실제 계정은 쓰지 않는다.

## macOS pipe 상속과 출력 대기(t-1104)

- `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib child_process::tests::pipe_window -- --nocapture`는 실제 std `pipe()` 반환 직전 창을 실험실 자식의 dyld interpose로 제어한다. 수정 전의 FD 상속·캡처 자식 종료 뒤 output 미완료·상속자 종료 뒤 반환과, 수정 후 std/Tokio/실제 harness 판 조회가 상속자 생존 중 반환하는 것을 함께 검사한다. stdout/stderr·실패 종료 코드도 보존한다. 꼭 필요한 이유: 가짜 지연이나 일반 병렬 spawn만으로는 macOS pipe 창을 확정할 수 없고, 잠금을 wait까지 잡는 새 직렬화 결함도 막아야 한다.
- 시험 C 코드는 실행 전에 저장소에서 검토하고 `cc -dynamiclib`로 자기 worktree target에만 만든다. 운영 실행 파일·설정에는 주입하지 않는다. socket·stdin·kqueue NOTE_EXIT 사건으로 끝내고 30초는 실패 상한으로만 쓴다. 고정 sleep·재시도·시한 연장은 없다. ignored `subprocess_fixture`는 이 회귀가 격리 자식으로 호출하는 보조 진입점이다.
- 관련 검사는 aaa-common/aaa-agent 유닛, host 사용량·machine_setup·quota/image RPC, session·CLI 업데이트와 의존 규칙을 사용한다. 전수 경로 표·OSS 비교·OS별 범위는 [자식 실행 조사](process-capture.md)에 있다. 새 E2E 번호·API는 없으며 전체 gate는 coordinator 차례에서 실행한다.

## 미답변 coordinator 질문의 자동 종료 방지(t-1090)

- 새 host 회귀 두 개는 실제 SQLite·post·질문 목록을 쓴다. `orphan_thread_reference_stays_a_coordinator_question_across_reports_and_reconcile`는 과거 답이 있지만 현재 카드가 없는 thread 접미사로 만든 요청이 독립 q-ID와 링크를 유지하고, 이후 질문 있음/없음 보고·정리에도 열려 있는지 검사한다. 꼭 필요한 이유: q-1372처럼 생성 직후 과거 답에 흡수되는 요청 유실을 막는다.
- `inbox_cleanup_preserves_unanswered_coordinator_questions_until_explicit_handling`은 구조화·평문 사용자 요청과 일반 알림을 섞어 `inbox done --all`이 일반 알림만 끝내는지, 독립 요청 ID를 직접 정리해도 거절하는지, 실제 답과 명시적 `post handled`는 끝내는지 검사한다. 꼭 필요한 이유: q-1373의 coordinator 정리가 사용자 답 없는 카드를 `gone`으로 만드는 경로를 막는다.
- 기존 E2E53의 닫힌 thread 후속 post를 독립 요청으로 남기는 계약으로 바꾼다. 실제 CLI·host·웹에서 카드와 q 링크, 일반 inbox 정리 뒤 유지, 명시적 처리 뒤 종료를 Chromium 1280px·WebKit 390/430px로 확인한다. 기존 열린 thread 카드의 알림 묶기·답 뒤 처리·보고로 닫힌 원래 카드와 늦은 응답의 두 엔진 × 1280/390px 계약은 유지한다.
- 부분 실행: `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib question_ids::tests`, `--lib asks::tests`, 기존 E2E53. 전체 ci-local은 coordinator의 차례를 받은 뒤 수행한다. 고정 sleep·재시도·시한 연장·단언 완화는 없다.
- 참고는 [Gitea 알림](https://github.com/go-gitea/gitea/blob/main/models/activities/notification.go)([MIT](https://github.com/go-gitea/gitea/blob/main/LICENSE))의 원래 대상 ID·읽음 상태 분리와 [Zulip 메시지 표시](https://github.com/zulip/zulip/blob/main/zerver/actions/message_flags.py)([Apache-2.0](https://github.com/zulip/zulip/blob/main/LICENSE))의 명시 대상별 상태 변경이다. 식별자와 알림 정리·답을 구분하는 원칙만 참고하며 외부 코드·의존성은 추가하지 않는다.

## 계정 이름 입력 중 부모 갱신(t-1088, §8.499 보완)

- 74회차5d088c34 gate의 E58 WebKit1280은 `renamed-3-ok` 행의 메일을30초 기다리다 실패했다. 남은 실험실에는 `renamed-3`가 실제로 저장돼 있었다. 별도 main a832f97a와 통합 실행 파일의 같은 E58·HTTP/native input 대조는 각각8.209/7.469초에 통과했다. 추가 관측에서 main WebKit1280도 같은30초 오류로 실패했고, native input의 `renamed-3-ok` → 입력 완료 뒤 `renamed-3` → 실제 account_rename의 옛 이름 전송/성공 → CLI/DOM의 옛 이름 순서를 기록했다. 이는 UI 표시 지연이 아니라 새 입력 유실이다.
- 계정 삭제의 accounts 사건과 계정/사용량 HTTP 갱신이 겹치는 기존 controlled 복원 경로다. 실제 「사용량 다시 읽기」 클릭을 input root capture에서 발생시키는 결정적 대조에서는 main과 통합 원판 모두 입력이 옛 값으로 돌아가 같은 잘못된 이름을 저장했다. 값을 재입력하거나 React state/host 응답을 조작하지 않는다. [React controlled input 안내](https://react.dev/reference/react-dom/components/input#controlling-an-input-with-a-state-variable)와 [ReactDOMUpdateBatching의 controlled 복원](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/ReactDOMUpdateBatching.js)([MIT](https://github.com/facebook/react/blob/main/LICENSE))을 비교했고, 저장소 ChatComposer·AsksView의 window native capture 방식을 적용한다. 외부 코드·의존성은 추가하지 않는다.
- 기존 E58의 Chromium/WebKit×1280/390 네 조합에 같은 부모 갱신을 넣고, 실제 사용량 요청·새 별칭 유지·같은 input DOM·한 번의 개입을 확인한 뒤 기존 충돌/성공·인증일/판/메일/plan·기본 참조·CLI 역변경·Reader 단언을 실행한다. 필요한 이유는 실제 계정 별칭이 사용자가 친 값과 다르게 저장되는 회귀를 막기 위해서다. 실패에는 엔진/폭·최초 오류/stack·실제 rename 요청 별칭을 추가 진단보다 먼저 파일에 남기고 입력/계정행/안내·개입 기록을 따로 붙인다. 기존30초 locator·20초 사건 한도와 이름/개수 단언은 그대로다. 순수 규칙을 바꾸지 않아 유닛을 더 늘리지 않고 기존 웹604개와 이 E2E를 확인한다.

## thread 목록 frame 상한과 페이지(t-1088, §8.502)

- host `orch::list_tests`는 실제 DB에 끝난 2,500건·준비/열림/실패/보류·다른 프로젝트의 9999/10000/10001을 넣는다. 모든 페이지의 실제 IPC 포장이 상한 안이고 2,507건이 중복·유실 없이 나오는지, 최대 200건·역할/상태/open 거르기·본문 축소·바이트로 먼저 나뉘는 상세 페이지·다음 행을 확인한다. 잘못된 커서·다른 프로젝트 커서·0 limit·상충 필터도 거부한다. 필요한 이유: 원장 증가에 따른 이번 frame 실패와 커서 경계 유실을 실제 저장/직렬화 경계에서 막는다.
- `front::tests::thread_pages_require_a_client_that_reads_cursors`는 실제 duplex 전송으로 1.127 IPC CLI에 갱신 오류가 오는지 본다. 필요한 이유: 커서를 버리는 구판이 첫 페이지만 전체로 오해하는 조용한 누락을 막는다. 기존 m13 `worktree_thread_round_trip`에는 resolve 뒤 기본 목록 보존·open 제외·resolved 선택만 더한다.
- 웹 `threadReload.test.mjs` 두 시험은 페이지 간 커서 전달·질문 상세 유지·같은 번호의 다른 프로젝트·수신 중 새 보고 보존, 뒤 페이지 오류·반복 커서·잘못된 응답을 검사한다. 필요한 이유: 전체 읽기를 페이지로 바꿔도 새 보고가 되돌아가거나 부분 목록을 성공으로 표시해서는 안 된다.
- 기존 E2E12 끝에 실제 끝난 thread 2,500건을 더한다. host 종료→DB transaction→재시작/agent 연결 사건을 기다리는 `seedHost`로 fixture를 쓴다. 1 MiB보다 큰 전체 목록을 실제 CLI·IPC에서 읽고 프로젝트 인자 유무·worker·open·resolved·limit/after를 확인한다. Chromium·WebKit을 headless로 열어 HTTP 각 페이지의 frame 크기·개수와 마지막 커서, 마지막 항목의 실제 표시를 확인한다. 필요한 이유: host 유닛이 대신할 수 없는 CLI의 페이지 합치기와 브라우저 초기/새로고침 경계를 검사한다. 새 E2E 번호는 만들지 않는다.
- 첫 대량 E2E는 제품 오류가 아니라 공용 `aaa()`의 Node `execFileSync` 기본 stdout 버퍼(1 MiB)에 걸려 자식이 끝 코드 null로 종료됐다. 이 시나리오 전용 CLI helper는 `spawn` stdout을 파일로 받아 종료 사건·코드와 전체 JSON을 확인한다. 버퍼/시한/기대값을 늘리지 않고 전체 결과가 1 MiB보다 크다는 단언도 유지한다.
- 부분 실행은 `cargo test --profile verify --locked -p aaa-host --lib orch::list_tests`, 같은 crate의 `thread_pages_require_a_client_that_reads_cursors`, `cargo test --profile verify --locked -p aaa --test m13 worktree_thread_round_trip`, `node --test web/test/threadReload.test.mjs`, `NODE_PATH=<자기 작업 폴더 Playwright 1.64.1> nice -n 15 node web/e2e/web.e2e.mjs --spec 12-threads-push --workers 1`이다. 공용 내부 브라우저 캐시와 저장소 launchChromium을 쓰며 고정 sleep·재시도·시한 확대는 없다. 전체 gate는 coordinator에게 준비됨을 알리고 받은 차례에 실행한다.

- 첫 전체 gate의 실제 m13-4 `automation_threads_leave_user_capacity_and_queue_slots_available`·E2E48은 목록에서 자동화 출처를 뺀 회귀였다. 원래 출처 단언을 그대로 유지해 고친 계약을 확인한다. 대량 fixture의 큰 자료는 질문 본문으로 옮기고 Next는 상세와 같은 값으로 보존해야 한다. 기대값을 줄여 출처 검사를 없애지 않는다.
- E2E103 fixture의 threads가 빈 한 페이지임을 요청/응답으로 확인했다. 본문 로딩·커서는 별도 pane_chat 경로다. 과거 페이지의 행마다 observer·기준 줄 보정을 반복하는 비용을 프로파일과 native resize/scroll 계측으로 조사하고 공유 observer·높이의 지연 이진 탐색으로 수리한다. 기존 E2E103의 Chromium/WebKit ×1280/390px·coordinator/일반 pane·각 3회·80ms 회선 지연·20초 대기와 픽셀 위치/요청 수 단언은 바꾸지 않는다. E2E29·41·104·109로 append·도구 묶음·숨김 수명·복귀 위치도 확인한다.
- 진단 계측은 실제 native ResizeObserver와 chat.scrollTop 쓰기를 세며 같은 150행 페이지에서 150 callback/304 writes가 1/5로, 가상화 후 36/76이 2~3/6~7로 줄었다. CPU 프로파일의 `holdAnchor` 반복과 함께 높이를 모두 읽은 뒤 이진 탐색하던 경로를 확인했다. thread 목록이 빈 fixture의 750→900행 commit 한 번만 CDP CPU 256배로 제한한 대조에서 기존 코드와 resize 묶음만 수리한 중간판은 원래 20초 `앞 쪽 commit` 오류로 실패했고, 높이의 지연 이진 탐색까지 수리한 판은 같은 코드·조건·대기 상한으로 통과했다. 256배는 실패 경계를 고정하는 진단용 극한 조건이며 원본 gate 당시 CPU 배분을 소급 확정하는 수치는 아니다. 시험 재시도·시한 확대·기대값 완화로 통과시키지 않는다. 이 느린 진단 조건은 정규 CI에 넣지 않는다.
- `chatScroll.test.mjs`에 4,096행을 실제 높이 accessor로 읽는 한 시험을 더한다. 앞 안내·중간·끝 밖에서도 원래 기준 줄/거리를 유지하고 높이 읽기 수가 전체 행에 비례하지 않아야 한다. 필요한 이유: 과거 기록이 쌓일 때 매 scroll마다 모든 줄의 layout을 강제하는 비용 회귀를 막는다. 기존 배열/잘림/빈 값/기억 시험은 그대로 둔다.

- E2E12 HTTP 수집은 문서·events 연결·첫 요청과 `next_after`로 구분한다. 초기 마지막 행만 기다린 뒤 reload 동안 모든 응답을 합치던 시험은 재연결의 새 전체 읽기도 더해 5,010≠2,505가 됐다. main `1f58f884`와 후보의 같은 5건 대조에서 각 연결 5건·화면 5건인데 합계 10건이고, 후보 2,505건 대조의 두 엔진은 각 연결 26페이지·고유 2,505건·끝 커서 없음인데 합계 5,010건이다. 요청마다 문서·연결·after, 응답마다 next·실제 ID를 기록했다. 페이지 안 중복이나 한 읽기의 커서 되감기는 없었다. main의 2,505건은 고치려는 기존 frame 상한 때문에 직접 비교할 수 있어 5건으로 같은 수집 경계를 대조했다.
- 정규 E2E12는 reload의 실제 마지막 host 응답을 보류하고 events 연결을 한 번 교체한 뒤 새 읽기 시작 사건에서 전달한다. SW가 Playwright HTTP 전달 gate를 우회하지 않게 이 목록 전용 paired context만 serviceWorkers=block으로 열고 실제 host·native WS 메시지를 그대로 잇는다. 관측한 응답을 버리거나 Set으로 줄이지 않으며, 독립 읽기마다 전체 2,505건·고유 개수·ID 차례·마지막 커서와 페이지 크기를 검사한다. 끝나지 않은 옛 읽기도 정확한 접두사여야 하고 같은 연결의 중복 첫 요청·선행 응답 없는 after·한 읽기 내 반복 커서는 실패한다. reload 문서의 두 완료 읽기와 DOM의 전체 항목이 각각 정확히 한 번인지 확인한다. 기존 frame·CLI 거르기/limit/after 단언과 대기 상한은 유지한다. 필요한 이유: 정상 재연결을 제품 페이지 중복으로 오진하지 않으면서 실제 중복 요청·커서 회귀를 잡는다. HTTP trace는 실패 산출물의 diagnostics에 보존한다. 수리판은 제품 소스가 같은 61a332fb 실행 사본과 최종 helper로 E2E12 전체 파일만 nice 15·worker 1개로 실행해 89.4초에 통과했다. 두 엔진 모두 초기 문서 1회·reload 문서 2회가 각각 26페이지·고유 2,505건이며 전체 표시도 고유 2,505건이었다.
- 74회차 재통합의 E2E12는 `thread-pages.mjs`의 `route.fetch: Request context disposed`가 처리되지 않은 Promise 오류로 child를 끝냈다. 실제 HTTP 응답을 보류한 최소 대조에서 context를 먼저 닫으면 같은 오류로 종료하고 최초 단계 오류가 가려졌다. 응답 보류 해제 → `context.unrouteAll({ behavior: "wait" })` → context 닫기 순서는 실행 중인 fetch/fulfill을 먼저 마쳐 원래 오류를 보존한다. 이는 [Playwright 공식 수명 계약](https://playwright.dev/docs/api/class-browsercontext#browser-context-unroute-all)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))과 기존 question-reply/machine-password helper의 정리 순서를 따른다. `ignoreErrors`로 handler 오류를 버리지 않는다. pairing 중 실패도 정리 범위에 넣고 최초 오류·stack·현재 단계를 정리 전에 HTTP trace에 기록한다. 정리 자체 실패는 따로 기록하며 원래 오류를 덮지 않는다. 최소 대조는 실제 HTTP 도착/route 완료 사건으로 끝나며 고정 sleep이 없다. E2E12 전체 단독은46.3초, 다른 E2E9worker와의 병렬 대조는49.9초에 통과했다. 두 실행에서 별도 최초 단계 오류는 나타나지 않았고 과거 gate에서 닫기를 촉발한 최초 오류는 소급 확정하지 않는다. 원래 20초 대기·26페이지·2,505건·두 완료 읽기·고유 ID/화면 개수·HTTP 바이트 단언은 유지한다.
- E2E37의 기존 12개 조합을 그대로 사용한다. 이진 탐색을 유지한 채 최초 실측의 동기 보정만 복구한 대조로 prepend·보관 복귀 회귀를 확인하며, 최종판은 이 동작과 공유 observer·같은 shell 목록 재사용을 함께 검증한다. E2E103 극한 CPU 진단의 실패 중간판도 보존한다.
- E2E115는 main `1f58f884` 원본(기본·Chromium CPU 4배)과 gate 후보(CPU 4배)를 분리해 대조한다. main 두 대조는 통과하고 후보는 같은 말풍선 중복으로 실패했다. 최종판은 Composer의 정리 effect 전에도 현재 기록에 수신된 낙관적 말을 그리지 않는다. 기존 MutationObserver의 매 전이 count≤1·sending·PTY 수신·ACK·reload 단언을 그대로 쓴다. `inputReceipt` 유닛은 예전 같은 글·다른 hash·미확인 값을 남기고 새 대기열/보통 수신만 같은 규칙으로 판정하는지 검사한다.

- E2E114의 자동화 입력은 host가 실제 DB를 바꾼 응답을 전달 gate에 붙잡은 뒤 focus 사건으로 실제 `routines` 갱신을 받는다. 목록의 막힘이 사라진 사건 뒤에도 기존 `responsive()`가 같은 진행 표시·취소/ESC 차단을 확인하고, 누르기 전후 확인 본문이 같은지 검사한다. 원래 500건·Chromium/WebKit ×1280/390px ×계속 보내기/버리기·단일 요청·DB 단언은 유지한다. main 원본과 후보 원판이 같은 30초 오류로 실패한 계측과 수리판의 pending 유지 기록을 보존한다. 목록 축소/페이지·대화 측정과 무관한 기존 자동화 확인창 수명 경합이며, 부분 실행은 E2E48·55·64·106·114를 사용한다.

- E2E113의 늦은 답 fixture는 동작 중 host DB에 직접 BEGIN IMMEDIATE를 하지 않는다. 초기 500건은 seedHost가 host 종료를 확인한 뒤 중첩 SAVEPOINT로 넣는다. E2E113의 보고서 초기 자료·탭 준비 UPDATE도 host가 멈춘 seedHost에서만 실행한다. 이후 검증 연결은 모두 readOnly이고, 열린 확인창 뒤 새 답은 실제 Post/AskAnswer API로 만든다. API의 사본 확정 뒤 시험용 그림 원본은 지워 뒤 폴더 빈 값 단언까지 유지한다. 기존 삭제 ID 500개·새 답 제외·DB 500→501→1→0·실패 복구·원문/그림 보존을 그대로 확인하며 새 답의 실제 host ID와 내용도 확인한다. 응답 gate는 fixture가 끝난 뒤 실제 삭제 요청에 건다. main의 진단 계측은 host 감사 INSERT 완료/commit 전과 Node fixture BEGIN의 PID·SQL·SQLITE_BUSY stack을 기록하며 고정 sleep 없이 FIFO 사건으로 끝낸다. 진단 계측은 제품에 남기지 않는다. 검증은 E2E113만 nice 15·worker 1개로 단독 실행한다. 시한 연장·재시도·기대값 완화는 없다.

## 활성 할 일 표시 정렬(t-1081, §8.497)

- `taskSortModel.test.mjs` 네 유닛은 각각 속성별 정렬·빈 값·0채움 번호·안정된 동률, 실제 예약/숨은 예약과 상하위/거르기 보존, 나중의 Markdown 표시 제목·메모·원문 위치 보존, 탭별 기억 분리·잘못된 저장값 복구를 검사한다. 필요한 이유는 실행 차례 훼손, 부모·하위 분리, 다른 항목을 옮기는 데이터 손실, 다른 탭의 보기 설정 훼손을 막기 위해서다. 정렬 상수만 복사하는 시험은 두지 않는다.
- 기존 E2E76에 Chromium/WebKit × 1280/390px를 더한다. 실제 host 예약/원문을 보존하며 다섯 속성 정렬·예약 시작 차례 고정·끌기/휴대폰 순서 도구 제거·Alt 이동 차단·행 메뉴와 상세의 예약 위치 이동 제거·예약 풀기 메뉴 유지·새로고침 기억·원래 차례와 이동 도구 복구를 검사한다. 320px에서도 정렬 칩 접근과 가로 넘침을 확인한다. 나중은 절 순서·제목/원문 차례·기억 분리와 정렬 뒤 메뉴에서 올바른 항목을 옮기는지 같은 네 조합으로 검사한다. 기존 손 차례 끌기·예약 추가/풀기·휴대폰 ▲▼ 계약은 그대로 유지한다.
- 필요한 이유: 유닛으로 확인할 수 없는 실제 키보드/메뉴/휴대폰 시트 경계와 host가 바뀌지 않는다는 계약을 기존 실험실 하나에서 확인한다. 첫 브라우저 검증은 백틱이 나중 제목 순서에 끼어드는 결함을 찾았다. 기존 Markdown 파서의 표시 글로 비교하도록 수정하고 해당 유닛에도 코드·강조·링크 제목을 넣는다. 고정 sleep·재시도·시한 확대·기대값 완화는 없다.
- 부분 실행은 `NODE_PATH=<작업 폴더 Playwright 1.64.1> node web/e2e/web.e2e.mjs <캡처 폴더> --spec 76,43,46,67,86,107 --workers 2`다. 내부 공용 브라우저 캐시만 사용한다. 전체 ci-local은 coordinator에게 「준비됨」을 알리고 실행 차례를 받은 뒤 한 번 수행한다.

- 73회차 gate 수리(t-1075): 이 owner의 두 상세 닫기→reload가 휴대폰 시트의 비동기 `history.back()`과 경쟁했다. 보존 로그와 같은 `ERR_ABORTED`를 실제 host·agent·Chromium 휴대폰에서 4배 CPU 지연 조건의 두 번째 반복에 재현했다. 순서는 reload 요청→history.back→요청 취소→popstate다. 정렬 값은 동기 localStorage 저장이라 저장 완료 문제가 아니다. 두 닫기 전에 popstate 대기를 걸고 그 사건 뒤에만 기존 reload·정렬 기억 단언을 실행한다. 같은 `#tasks` 주소 안의 기록 복귀이므로 waitForURL로 대체하지 않는다. 실패에는 엔진·기기를 붙인다. 기존 20초 사건 상한·30초 reload 상한·단계 상한과 모든 단언은 유지한다.
- 검증: 제품 탐색·HTTP 응답을 바꾸지 않은 같은 4배 CPU 지연에서 수정 전 2회째 실패, 수정 후 100/100회 통과하고 매번 popstate가 reload보다 앞섰다. 반복은 임시 조사 도구에만 두고 영구 시험 수는 늘리지 않는다. 실제 두 엔진×데스크톱/휴대폰을 포함한 E2E76 전체도 35.2초에 통과했다(관측 load 26.61→42.05). 사건 대기는 기존 E2E39의 popstate 원칙과 [Playwright navigation 안내](https://playwright.dev/docs/navigations#waiting-for-navigation)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))를 참고했으며 외부 코드는 가져오지 않는다.

## workspace 기준 실행의 시간 추이 표시(t-1082, §8.496)

- `node --test web/test/workspaceSelection.test.mjs` 두 유닛은 시각별 ID와 날짜별 p50 묶음(대표 ID가 다른 표본 포함), UTC/KST 자정 경계·머신/profile 분리·실패 제외, 필터 밖·결측과 실제 0초의 구분을 확인한다. 꼭 필요한 이유: p50을 기준 실행의 실제 시간으로 오해하게 하거나 미수집을 0으로 그리는 회귀를 막는다.
- 기존 host `reader_queries_only_the_host_ledger_with_filters_missing_values_and_disconnected_agents`에 40일 전 실행의 조회를 더했다. 기간·profile·머신 밖 기준 실행은 원장에서 찾되 runs/추이에 섞지 않고, 다른 workspace의 ID는 반환하지 않아야 한다. 필요한 이유: 웹 캐시로 유지하면 새로고침·기간 밖 조회가 깨지고 다른 저장소 자료를 섞을 수 있기 때문이다. 별도 실험실·시험 개수를 늘리지 않는다.
- 기존 E2E90의 실제 보고→agent 수집→host 원장 fixture 네 실행을 성공/실패, 빌드 0초/미수집, 이틀 전 실행으로 구성한다. 기존 밝음/어두움·1280/820/390/320px·Chromium/WebKit 매트릭스를 유지하며 밝음 1280/390px 네 조합에 기준 실행 검사를 덧붙인다. 실행 두 개 선택 뒤 안내선·강조점·원본 점의 좌표 일치와 이동, 검증/빌드 전환, 실제 0초·미수집, 오늘 밖/7일 복귀, 실패 p50 제외와 비대표 실행의 p50 포함, UTC/KST 전환, 머신 밖·새로고침 유지, 키보드 상세와 가로 넘침을 확인한다. 꼭 필요한 이유: 실제 필터 응답과 URL 복원·두 SVG 엔진의 표시를 유닛으로 대신할 수 없다.
- 실행: `NODE_PATH=<작업 폴더 Playwright 1.64.1 모듈> nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 90 --workers 1`. 브라우저는 내부 공용 캐시와 저장소 `launchChromium` helper를 쓴다. 고정 sleep·실패 재시도·시한 연장·기대값 완화는 없다. 전체 ci-local은 coordinator가 준 차례에 한 번 수행한다.

## IME 연속 조합과 한 controller(t-1075, §8.498)

- `composition-anchor.test.mjs`는 빌드에 복사하는 실제 controller를 실행한다. 기존 두 시험은 분리 출력의 시작 위치와 줄 marker 수명을 유지한다. 새 시험 하나는 이전 확정 에코 전 다음 음절 시작, timer 전/후의 가나다라, 받침 이동 각→가가와 WebKit의 NBSP→SPACE 정규화를 함께 검사한다. 꼭 필요한 이유: 이전 시험은 다음 조합 전에 커서를 직접 새 칸으로 옮겨 이번 회귀 경계를 만들지 못했다.
- 기존 모바일 시험은 같은 controller의 iOS 영어 마지막 글자·자동수정·한글 보류·화면 읽기·지연 Backspace를 유지한다. 확정 직후 일반 insertText의 beforeinput이 먼저 오면 DOM 편집 전 앞 큐를 비우는 순서를 기존 순서 시험에 더한다. native Backspace·finishInput의 확정 한 번/marker 해제도 기존 controller owner에서 검사한다.
- E2E09의 「한글 조합 위치」 owner를 확장한다. 같은 실제 host·agent·raw PTY에서 확정 바이트를 기록하고 명시적인 flush 사건 전까지 echo를 보관한다. 가나다라+space와 각→가가에서 다음 조합을 먼저 시작하고, echo 전/후·상태 줄로 이동한 분리 출력 모두의 위치를 검사한다. 시작 좌표·실제 빈 커서 칸 폭을 비교하며 확정 문자열·순서·중복 없음도 검사한다. native WebKit은 한 NSTextInputClient 명령 안에서 commit→setMarkedText를 호출한다. Chromium은 CDP, Playwright WebKit은 기록 재생이다.
- `AAA_IME_CAPTURE=<폴더> NODE_PATH=<Playwright 1.64.1 경로> node web/e2e/web.e2e.mjs --spec 09 --grep '한글 조합 위치' --workers 1`은 단계 PNG와 사건/rAF JSON을 보존한다. 실패도 해당 엔진 trace를 남긴다. 고정 sleep·재시도·시한 확대가 없다. 부분 확인은 기존 E2E02·07·09·15다.
- 사용자 추가 지시의 실기 확대는 임시 fixture로 macOS native 키·OS input context의 현재 source, 실제 PTY의 취소·space/Enter·영문·paste·pane 전환·resize와 격리 iPhone native WebKit을 확인하고, 결과·불가능한 경계를 작업 보고에 남긴다. 외부 OS 입력기를 거치지 않는 marked text 교체와 일반 키 주입을 실제 한국어 2벌식 키 사건이라고 쓰지 않는다. 영구 E2E 번호/대형 실험실을 늘리지 않는다.

## 대화 보낸 말의 즉시 표시(t-1069)

- E2E115는 실제 host·agent·PTY와 Claude 감지기의 가짜 harness(작업 중 화면, `.queue` 표지가 있으면 기록 대기열 줄)를 쓴다. Chromium/WebKit × 1280/390px마다 첫 말은 submit ACK를 Playwright WebSocket route에서 붙잡아 ACK 전 `보내는 중` 말풍선·빈 입력창을 사건으로 확인하고, 풀면 `대기 중`, 같은 탭 새로고침 뒤 유지, 기록 줄 뒤 보통 말을 본다. 둘째 말은 붙잡지 않고 MutationObserver 기록으로 Enter부터 각 단계까지의 ms와 같은 글 말풍선이 하나뿐인지 확인한다.
- 꼭 필요한 이유: 사용자가 보낸 것을 바로 보는 계약(ACK 전 표시)과 중복·새로고침 유지는 단위 시험으로 실제 WebSocket·기록 경로를 대신할 수 없다. E2E23 막힘 단계에는 말풍선의 막힘 표지와 멈춘 뒤 거둠 단언만 더했다.
- 실행: `NODE_PATH=<작업 폴더 Playwright 1.64.1> node web/e2e/web.e2e.mjs <캡처 폴더> --spec 115 --workers 1`, 관련 `--spec 23,35,102,45,22,25,11,66`. 고정 sleep·재시도·시한 연장은 없다.

## Windows M4의 내부 profile 연결(t-1072)

- `scripts/ci-windows-sshd.ps1`은 workflow의 성공한 ci-local build가 만든 `target/e2e/aaa.exe`를 사용자·서비스·보호 입력 생성 전에 확인한다. `windows-sshd-test-command.ps1`의 같은 준비 함수가 절대 binary 경로·worktree target·작업 폴더와 `cargo test --profile verify`를 정한다. 다른 target이나 debug 캐시로 대체하지 않으며 preflight SHA와 profile·산출물 경로·시험 종료 코드를 기록한다.
- 꼭 필요한 새 회귀 `pwsh -NoProfile -File scripts/test-windows-sshd-command.ps1`은 Windows ci-local test에서 실행한다. 임시 폴더의 native cargo fixture를 rustc로 만들고 실제 Process.Start로 argv·환경·공백 포함 작업 폴더·fresh e2e bytes를 검사한다. 첫 실행에는 debug 폴더 자체가 없다. 이어서 e2e 파일을 없애고 옛 debug 파일만 두면 준비 단계에서 거부해야 한다. 서비스·사용자·SSH·설치는 실행하지 않는다. 이 회귀는 이번의 빌드/시험 연결 누락과 캐시가 누락을 가리는 회귀를 함께 막기 위해 필요하다.
- 전체 SYSTEM SSH·사용자 SCM 설치/등록·틀린 암호/보호 입력·TLS 연결·영속 원문 부재·정리는 기존 ignored Rust owner가 그대로 맡는다. 운영 머신에서 GitHub 전용 guard를 우회하지 않는다. 비특권 회귀 통과를 전체 M4 통과로 세지 않는다.
- macOS 실제 LaunchAgent와 root SSH의 ignored owner도 `--profile verify`로 사전 컴파일과 맞춘다. `CARGO_BIN_EXE_aaa`가 해당 profile 산출물을 선택한다. 관측하지 않은 macOS CI 실패를 이번 원인으로 확정하지 않는다. 실패 재시도·시한 확대·기대값 변경은 없다.
- OSS 참고는 [uv의 내부 fast-build profile](https://github.com/astral-sh/uv/blob/main/Cargo.toml)(MIT OR Apache-2.0)과 [Helix의 integration profile](https://github.com/helix-editor/helix/blob/master/Cargo.toml)·[명시적 integration-test alias](https://github.com/helix-editor/helix/blob/master/.cargo/config.toml)(MPL-2.0)다. 내부/배포 profile 분리와 실제 시험 호출의 profile 명시를 따르며 외부 코드·의존성을 추가하지 않는다.

## 할 일 완료 목록 정렬(t-1067)

- host 유닛 `archive_sorts_newest_first_and_pages_across_equal_values`: 끝난 날짜 기본·시작 날짜(처음 이은 thread, 기록 없음은 끝)·번호의 내림차순, 같은 끝난 시각을 한 건 페이지로 넘겨도 건너뛰거나 되풀이하지 않는 커서, 옛 ULID 커서 거부, 시작 시각을 내구 보관에 쓰지 않음을 검사한다. 기존 `archive_search_pages…`는 기본 차례 기대만 최신순으로 바꿨다.
- 웹 유닛 `taskArchiveModel.test.mjs`: 기기 시간대 자정 경계의 날짜 묶음·「시작 기록 없음」 끝 묶음·번호순 무묶음과 24시간제 시각. fixture 날짜가 1970년 초인 E2E가 대신 볼 수 없는 오늘/어제/다른 해 머리를 고정한다.
- 기존 E2E86에 정렬 단계를 더한다: 실제 host 130건 fixture의 넷 중 셋에 thread 이력을 넣어 세 정렬의 차례가 서로 다르다. 화면 차례가 host 응답 차례와 같고 host 응답이 정렬 값 내림차순(같은 값은 큰 번호)인지, 「더 보기」 두 번으로 전체가 같은 정렬로 이어지고 마지막 묶음이 「시작 기록 없음」인지, 번호순에 날짜 머리가 없는지, 탭을 다시 그려도 기기에 고른 정렬이 남는지 본다. 행 구조 변경으로 E2E86·47·67과 E2E26(panel-scroll)의 펼침을 `summary` 대신 펼침 단추로 누르고 모델·머신 확인을 펼친 상세에서 한다. 320·390px 가로 넘침 검사는 그대로다.
- 전후 화면은 같은 40건 fixture(10일에 걸친 끝·시작 시각, 저장소·thread·근거)로 Chromium·WebKit × 1280/390px headless 촬영했다(커밋하지 않은 임시 spec). FRONT_API 1.126, E2E 번호 추가 없음.
## brief 확인보다 먼저 회수한 완료 보고(t-1066)

- 보존된 실패 DB의 사건은 `report_ready` → `brief_delivered` → `review_acked`였다. 전달 확인 때 이미 도착한 새 보고 hash를 `delivered_report`에 다시 넣어, ack 뒤에도 15초 working 유예가 남았다. 화면 멈춤을 3초로 두는 기존 우회는 이 순서를 보장하지 못한다.
- 비교 기준은 첫 전송 직전 DB 갱신에서 잡고, 재전송·Enter 재전송·늦은 받음 확인은 바꾸지 않는다. 전송 전에 거부돼 시도 수가 0으로 돌아간 경우와 새 brief의 첫 시도는 그때의 보고로 새 기준을 잡는다. 전달 확인 시각·제품 유예 길이·ack 계약은 유지한다.
- 꼭 필요한 새 회귀 `orch::report_tests::brief_confirmation_preserves_reports_collected_after_enter`는 실제 Host DB·보고 수집·brief 제출·ack 경계를 사용한다. fake Agent link가 Enter 처리 뒤 응답만 보류하고, 실제 보고 수집을 응답 전/후로 제어한다. 최초 시작/기존 보고가 있는 재시작 × 보고 먼저/확인 먼저/Enter 응답 유실 뒤 화면 수동 재확인 여섯 조합에서 새 보고의 검토 대기·ack 뒤 idle·hash·정확한 사건 순서를 확인한다. 옛 보고 ack만으로 새 brief 유예가 끝나지 않는 반대 조건도 지킨다. Tokio 가상 시각을 써서 유예가 저절로 지난 통과를 막는다.
- 수정 전 새 회귀는 `report_first=true, previous=None`에서 `Working` / `Idle`로 실패했다. 기존 m13의 `worktree_thread_round_trip`은 300ms 공통 실험실로 돌리고 idle 단언을 그대로 유지한다. 독립 경로인 보고·brief 두 사건만 순서를 정렬하며, 전체 사건 수·시작/ack/resolve의 순서는 정확히 검사한다. `memory_and_restart`는 이미 공통 실험실과 pane 종료 뒤 idle을 검사하므로 제품 수리를 함께 검증한다.
- 참고는 [Tokio Notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))의 사건 신호와 [Loom의 순서 제어](https://github.com/tokio-rs/loom)([MIT](https://github.com/tokio-rs/loom/blob/master/LICENSE))다. 순서를 제어해 경쟁을 증명하는 원칙을 따르며 코드·의존성은 추가하지 않는다. 시한 연장·고정 sleep·실패 재시도·idle 기대값 완화는 없다.

## thread 질문 중복 카드 방어(t-1063)

- 새 host 유닛 `post::tests::plain_user_requests_link_only_one_current_thread_question`은 실제 post 접수·DB에서 제목 t-ID/본문 q-ID/0채움 별칭의 자동 연결과 원래 입력 재전송을 검사한다. 필요한 이유: 같은 질문의 알림이 새 카드가 되는 회귀와 잘못된 질문에 연결되는 데이터 손실을 함께 막는다. 별개 구조화 질문·명시 연결·다른 프로젝트·옛 q-ID·답한/닫힌 카드·부분 문자열 보존, 여러 카드/제목 한도 거부도 검사한다.
- 기존 E2E53에 연결 없는 평문 확인 필요 post를 더한다. 필요한 이유: 실제 CLI → host → Chromium/WebKit의 「답할 것」이 한 카드·한 배지를 유지하고 카드 답 뒤 자동 연결 post도 한 번만 닫히는지 확인해야 한다. 기존 명시 연결 post 두 개와 원문/그림 보존·권한·답 기록 단언은 유지한다.
- 기존 m13 `doctor_files_brings_the_coordinator_guide_to_existing_projects`의 안내 계약에 연결 접미사와 새 질문/선택지 금지를 더해 신규 생성·doctor 갱신에서 규칙이 빠지지 않게 한다. 새 실험실이나 시험 수를 추가하지 않는다.
- 고정 sleep·시한 확대·재시도·기대값 완화가 없다. Playwright 1.64.1 패키지만 자기 작업 폴더에 두고 내부 공용 브라우저를 사용한다. 전체 ci-local은 coordinator가 부여한 차례에 실행한다.
- 첫 전체 gate는 396초에 웹 유닛 584개·fmt·build·clippy·전체 E2E·public이 통과했지만 기존 `task_board_reads_tasks_and_follows_file_edits`가 실패했다. `std::fs::write`의 truncate→write 사이에 ticker가 빈 파일 SHA를 먼저 내보낸 것이 원인이다. 해당 fixture의 두 직접 쓰기를 인접 `task_edit_keeps_bytes_conflicts_and_logs`와 같은 pending 파일 작성→tick→rename으로 바꾼다. 빈 파일 사건을 건너뛰거나 SHA 기대값·시한을 바꾸지 않고, 완성 파일의 첫 관측 사건이라는 기존 계약을 유지한다.
- OSS 참고: [Gitea 알림 원장](https://github.com/go-gitea/gitea/blob/main/models/activities/notification.go)([MIT](https://github.com/go-gitea/gitea/blob/main/LICENSE))의 같은 이슈 대상 알림 갱신, [Zulip 주제](https://docs.zulip.com/help/introduction-to-topics)([Apache-2.0](https://github.com/zulip/zulip/blob/main/docs/contributing/licensing.md))의 대화 주제 묶기를 비교했다. 기존 질문 카드를 정본으로 삼는 방식을 따르며 외부 코드·의존성은 추가하지 않는다.

## 상태 줄 출력 중 한글 조합 위치(t-1064, §8.493)

- 실제 xterm 소스에 배포와 같은 패치를 붙이는 `composition-anchor.test.mjs`의 첫 유닛은 구판의 상태 줄 이동과 수정판의 시작 칸 유지, 확정 한 번·다음 조합의 새 위치·reset 해제를 대조한다. 둘째 유닛은 스크롤·열 축소·다른 버퍼·삭제된 줄의 위치와 숨김/해제를 검사한다. 입력 큐/모바일 시험에도 새 패치를 적용한다. 두 유닛은 위치 고정 회귀와 marker 수명 누수를 각각 막는 데 필요하다.
- 기존 E2E09에 실제 pane의 한 묶음/분리된 상태 줄 갱신을 추가한다. 브라우저 rAF마다 미리보기의 text/active/display/visibility/opacity·화면 좌표와 textarea 좌표를 기록하고 시작 위치 1px 이내, 확정 전 PTY 전송 0회·확정 뒤 `한` 한 번 수신을 단언한다. 출력/그리기 사건을 확인한 뒤 다음 묶음을 보내며 고정 sleep·시한 확대·재시도는 없다. 이 E2E는 실제 host/PTY/브라우저의 분리 출력 경계와 aaa의 행 보정이 유닛 밖에서 회귀하지 않도록 필요하다.
- Chromium은 CDP `Input.imeSetComposition`으로 ㅎ→하→한을 유지한다. macOS에서는 화면에 올리지 않는 WKWebView의 NSTextInputClient `setMarkedText`로 같은 실제 조합을 만들고, compositionstart/update의 trusted 값을 확인한다. Playwright WebKit은 기존 방식의 DOM 사건 재생으로 별도 검사한다. Linux에서는 macOS native 실행만 없으며 이를 실제 WebKit IME 확인으로 세지 않는다. OS 입력기 설정·입력 장치·사용자 창은 바꾸지 않는다.
- `AAA_IME_CAPTURE=<폴더> NODE_PATH=<작업 폴더 Playwright 1.64.1> node web/e2e/web.e2e.mjs --spec 09 --grep '한글 조합 위치'`는 단계별 화면과 frame/사건 JSON을 남긴다. 수정 전 Chromium·WebKit 실제 조합에서 상태 줄로 이동하는 RED, 수정 후 같은 출력·위치 기준의 GREEN을 확인했다. 비교 영상은 실제 브라우저의 단계별 캡처를 0.5초씩 확대 재생한 것으로 실제 입력 속도 영상과 구분한다.
- 부분 검사는 E2E02·07·09·15와 웹 유닛이다. 전체 ci-local은 머신의 차례를 받은 뒤 한 번 수행한다. xterm.js/VS Code/Ghostty(MIT) 비교와 제품 결정은 plan §8.493에 있다.

## 할 일 한 줄 배치(t-1048)

- 기존 E2E107을 확장한다. Chromium 1280/2000/390/320px·WebKit 1366/1024/390/320px에서 실제 host의 일반·예약·하위·병합 대기 행을 표시한다. 목록의 실제 너비가 640px 이상이면 모든 표시 칩과 제목·메뉴 중심이 1px 이내이고 행 높이는 40px 이하여야 한다. 좁은 목록은 제목 아래 칩 시작점, 휴대폰 제목 100px·행 높이 76px·메모만 있는 행 40px 계약을 유지한다.
- 기존 겹침·잘림·공통 열·하위 들여쓰기·첨부·가로 넘침·상세 열기/닫기·폭 전환 뒤 표지 한 개 검사에 실제 TASKS.md의 긴 제목 갱신 뒤 말줄임·겹침, 상세를 열어 목록이 좁아진 뒤의 배치, 제목 원문 tooltip과 메뉴 열기를 더한다. 변경 전후 1280px·390px 화면은 같은 실제 fixture로 headless 촬영한다. 긴 제목의 파일 갱신은 기존 ticker tick으로 알리고 DOM·글꼴·host 사건으로 끝낸다. 고정 sleep·재시도·시한 확대는 없다.
- 부분 검사는 E2E107과 기존 할 일 E2E43·46·47·52·54·67·70·75·76을 사용한다. 전체 ci-local은 coordinator가 준 차례에 한 번 실행한다. 작업 폴더의 Playwright 1.64.1 패키지와 내부 공용 브라우저만 쓰고 시스템 설정을 바꾸지 않는다. FRONT_API 1.125·기존 E2E 번호를 유지한다.
- OSS 후보는 [herdr-web-ui Sidebar.css](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/Sidebar.css)의 축소 가능한 제목과 후행 칩([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)), [herdr-projects popup.rs](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/popup.rs)의 제목·담당·thread·메모 순서([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE))다. 구성만 참고하고 외부 코드·의존성은 추가하지 않는다.

## E2E93 고유 계약과 중복 작업 축소(t-1050)

- E2E93의 직접 실행·재개 × Codex/Claude × Chromium/WebKit × 1280/390은 같은 backend 감지와 공통 웹 동작을 16개 독립 실험실에서 반복했다. 아래 두 수명 작업으로 줄인다. 같은 pane/기록을 직접 실행→교체→재개로 이어 중복 pane·기기·기록 준비도 없앤다. 일반 웹 조합은 기존 E2E07/30/37/103/104/109가 계속 검사하며 E2E 번호·제품 코드·공통 runner는 바꾸지 않는다.

| 남긴 독립 작업 | 고유 계약 |
| --- | --- |
| Codex 수명 · Chromium 390 | inline 기록·사본 → 열린 FD/같은 ID 백업·늦은 첫 기록 → 실제 터치/1px 앵커 → 보는 중 파일 교체 → resume·프로그램 입력/클릭 → PID 재발견·새 cursor |
| Claude 수명 · WebKit 390 | inline 기록·사본 → PID 등록·늦은 첫 기록 → native 1px 앵커 → 떠난 동안 300→360개 새 파일 → resume/continue·입력/클릭 → PID 재발견·새 cursor |

| 기존 단계의 계약 | 남긴 검사·대신 맡는 기존 시험 |
| --- | --- |
| 파일 전 연결 추측 금지·늦은 파일 자동 연결 | 두 수명 안의 직접 실행. agent `shell_chat`의 FD/중복/하위 agent/root 밖 기록·PID/시작 시각/interactive 판정도 유지 |
| 같은 UUID 백업 대신 실제 파일 읽기 | Codex 수명, agent `chat_paths_tests::codex_chat_uses_the_exact_bound_file_even_when_the_id_has_a_backup_copy` |
| 처음까지 페이지 읽기·갱신에도 읽던 자리 유지 | 두 수명의 360개 전부, E2E37의 2400개+400개 갱신, agent `chat_pages::pages_reach_the_first_block_while_the_file_grows` |
| 안내 1px 성장 때 맨 위 0·native 1px 입력 뒤 줄 고정 | 두 수명의 실제 DOM 단언, `chatScroll.test.mjs`의 안내 높이·앵커 유닛 |
| Chromium 실제 터치 뒤 맨 위 복귀·append | Codex 390에서 touch/scrollend와 정확히 0 유지. WebKit 390은 기존 DOM scroll 경로 |
| 같은 프로세스의 세션 교체 때 옛 본문·위치 제거 | 보는 중 FD 교체는 Codex 한 번, 숨김 중 PID 등록 교체·부분 파일 복귀는 Claude 한 번. 같은 웹 초기화 계약의 중복 전환만 제거 |
| 연결 전 alternate 화면 복귀 | 최초 inline 기록/사본 복귀와 통합. 일반 alternate 사본·복귀는 E2E07/109가 유지 |
| pane 왕복 보기 선택·읽던 첫 메시지·입력창 고정 | 재개에서 각 수명의 재시작 뒤 한 번. 일반 위치/초안/보관 DOM 복귀는 E2E30/37/103/104 |
| inline DECSTBM 기록·사본 복원 | 최초 직접 실행의 inline 검사, aaa-vt의 스크롤 영역/재생 유닛과 E2E07 |
| 재개 프로그램 스크롤·footer 고정·복귀 | 세 mode 각각 휠/복귀 두 번을 한 번으로 축소. 반복 복귀·실제 PTY 휠은 E2E109, 양 엔진·휴대폰 입력은 E2E07 |
| 클릭 누름·뗌 뒤 quit·종료 출력·정리 | 세 mode 모두 실제 클릭과 입력 trace의 M/m·quit·exited를 유지. agent `link::resume_tests::requests_read_before_host_publication_keep_session_replies_and_exit_output`은 등록 전 구독/종료 응답 유실도 검사 |
| 저장 연결 삭제 뒤 재시작·PID 재발견 | Codex resume 한 번, Claude continue 한 번. 같은 registry/FD 경로의 Claude resume 중복 재시작만 제거 |
| 재시작 뒤 새 history commit·cursor로 처음까지 재독 | 각 harness에서 재시작 전 옛 「처음이다」를 보관한 뒤 새 history commit을 확인하고 다시 처음까지 읽는다. 일반 페이지 재독의 mode별 반복만 제거 |
| 셸 복귀 때 연결·대화 단추 제거 | 두 수명. agent `shell_return_cannot_restart_released_harness_for_either_poll_order`, host `shell_harness_replacement_clears_the_previous_chat_identity`도 유지 |
| 가로 넘침 없음 | 두 수명과 기존 여러 화면 E2E |

- 고정 sleep은 없었다. 주 비용은 실험실 16개와 같은 감지 주기·전체 기록·왕복·재시작의 반복이다. 꼭 필요한 실제 셸/agent 기동은 남기고, 독립 실험실은 두 개, 각 실험실은 pane/기기 한 개와 직접 실행에서 만든 360개 기록을 재개에서도 사용한다. 재시작은 각 harness 한 번씩이다. 기존 360개 기록·정확한 좌표/위치·DOM 20초·locator 30초·단계 120초·완료/오류 단언을 유지하며 실패 재시도·시한 연장·기대값 완화는 없다.
- 전후 비교는 같은 e2e 실행 파일·웹 번들·Playwright 1.64.1·공용 브라우저 캐시·`nice -n 15`·worker 4개로 `node web/e2e/web.e2e.mjs --spec 93 --workers 4`를 실행한다. runner의 파일별 독립 작업 합계(준비/정리 포함)와 전체 wall time을 구별한다. `--write-durations`는 성공한 최종 후보의 합계를 기록한다. 전체 ci-local은 coordinator가 부여한 순서에 한 번씩 실행한다.
- 같은 조건의 단독 비교에서 기존 16/16은 독립 작업 합계 254.708초·wall 74.447초, 두 수명 2/2는 합계 52.796초·wall 28.418초였다(합계 79.3% 감소). 모든 준비/정리 시간을 포함하며 기동 중 다른 전체 CI 부하는 별도로 통제하지 않았다. 335초 과거 화면과 이번 단축률을 섞지 않는다.
- 참고 후보는 [Playwright의 사용자 동작·fixture 격리](https://playwright.dev/docs/best-practices)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [Cypress의 명시 조건/응답 대기](https://docs.cypress.io/app/core-concepts/best-practices#Unnecessary-Waiting)([MIT](https://github.com/cypress-io/cypress/blob/develop/LICENSE)), [xterm.js MouseTracking 시험](https://github.com/xtermjs/xterm.js/blob/5.5.0/test/playwright/MouseTracking.test.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))이다. 격리와 입력/출력 계약별 시험 구성을 참고하며 외부 코드·제품 의존성은 추가하지 않는다.

## 영구 삭제의 공개 경로 분리와 배경 제거(t-1042 후속)

- 67회차 착지 t-1043의 전체 부하에서 기존 후보는 WebKit 1280px의 500개 삭제 HTTP 12.508초로 10초 목표에 실패했다. host 계측은 파일 제거 11.870초·잠금 0.211초·DB 약 7ms였다. 앞선 회차의 7.738초는 충분한 여유가 아니었으며 아래 초기 DB 개선의 통과 결과를 착지 성공으로 보지 않는다.
- 보호 판정과 공유 링크 폐기를 먼저 끝내고, 호스트 사본 디렉터리를 같은 프로젝트 볼륨의 숨은 삭제 대기 batch로 rename하고 작은 manifest·t/r 본문은 직접 제거한다. 다른 볼륨 오류는 복사/삭제로 우회하지 않는다. 공개 경로에서 분리한 사본만 원장에 확정하고 실제 제거는 전용 작업자 한 개에 넘긴다. 파일 이동·DB·잠금 대기와 배경 정리 경과를 별도로 기록한다. 목록·사용량 조회는 삭제 대기를 재귀 스캔하지 않는다.
- 대기 폴더는 host DB 식별자로 구분한다. 기동 시 이 host가 소유한 batch만 회수하며 다른 state의 대기·불명한 이름·symlink/junction은 지우지 않는다. 배경 오류에는 경로·OS 번호·원인을 남기고 batch를 다음 기동에 보존한다. 살아 있는 worker가 실패를 자동 반복하지 않는다. Host 해제는 자신의 작업자 종료를 확인한다.
- 공개 경로 이동도 순차일 때 단독 전체 gate에서 최댓값이 8.458초여서 여유가 부족했다. 요청당 작업자 네 개로 한 원본과 보존 판의 독립 경로를 함께 정리한다. 사본 디렉터리의 목적지는 batch 아래 네 칸으로 나누고 작은 메타 파일에는 rename을 추가하지 않아 목적지 잠금 경합과 파일시스템 작업 수를 줄인다. 그 보고서의 모든 작업이 끝난 뒤 성공 사본만 확정하며 오류 순서는 입력 순서로 유지한다. 다음 보고서는 앞 보고서가 모두 끝나야 시작하므로 실패 뒤의 보고서 파일은 건드리지 않는다. 채널 회귀는 두 이동의 동시 시작과 한 경로가 실패해도 다른 경로의 종료를 기다리는 계약을 고정 시간 대기 없이 검사한다.
- host 회귀는 채널 사건으로 실제 제거를 막고도 요청이 완료되는지 확인하고, 같은 이름으로 재발행한 새 파일이 옛 batch 정리의 영향을 받지 않는지 검사한다. 배경 제거 오류·재기동 회수·다른 host state 분리·불명한 폴더 보존·대기 root symlink 거부를 검사한다. 기존 링크 폐기 rollback·파일 중간 오류·완료 사본만 확정·결과 DB 실패 후 복구·진행 중/보관 참조 보호와 성공 묶음 재전송 방지 단언은 유지한다.
- E2E113은 기존 네 화면의 500개 HTTP 10초 미만 단언을 그대로 실행한다. 각 화면 끝에는 fs.watch의 폴더 사건으로 실제 대기 batch가 모두 제거되는지 추가 확인한다. 실제 제거 확인도 기존 20초 응답 확인과 같은 상한을 쓰며 고정 sleep·재시도·시한/대상 수/기대값 완화는 없다. E2E114와 함께 전체 ci-local 10 worker 부하로 검증한다.
- 참고 후보는 [libuv의 파일 작업과 rename](https://github.com/libuv/libuv/blob/v1.x/src/unix/fs.c)([MIT](https://github.com/libuv/libuv/blob/v1.x/LICENSE))과 [tempfile persist의 동일 파일시스템·실패 반환](https://docs.rs/tempfile/latest/tempfile/struct.TempPath.html#method.persist)([MIT](https://github.com/Stebalien/tempfile/blob/master/LICENSE-MIT))이다. 빠른 이름 변경과 느린 물리 제거의 수명을 나누고 실패 시 원본/대기를 보존하는 원칙을 참고하며 외부 코드·의존성을 추가하지 않는다.

## 보고서 500개 영구 삭제의 묶음 DB 처리(t-1042)

- 기준은 t-1038·t-1041이 포함된 757a8110이다. 격리 Chromium 두 폭의 기존 500개 요청은 HTTP 646/342ms, 삭제 실행 248/249ms였다. 1,000개 사본의 파일 작업 합계는 130/130ms, 사본별 DB 제거는 57/58ms였다. 나머지 실행 시간은 판 삭제 표시·감사 기록 등이며 HTTP와 실행의 차이에는 반복 검증/조회도 포함된다. 이는 부하 없는 원인 분리 표본이며 이전 전체 gate의 12.2~16.5초나 개선 뒤 전체 gate 결과를 대신하지 않는다.
- 프로젝트별 catalogue·보관 참조 조회 뒤 전체 검증을 끝낸다. 링크 폐기와 파일 제거 후 결과 확정을 각각 한 transaction으로 묶고 prepared statement를 재사용한다. 파일 삭제 동안 DB transaction을 유지하지 않으며 실시간 원본·미완료 사본·번호 원장과 다른 프로젝트를 보존한다. 공유 링크·앱 열람 권한은 파일에 도달하기 전에 무효화한다.
- DB 유닛은 두 번째 링크 폐기와 두 번째 감사 기록에 SQLite trigger 오류를 주입해 두 transaction의 전체 rollback·원인 보존·프로젝트 분리를 검사한다. host 유닛은 원본/r 별칭 중복 제거, 누락 대상을 포함한 전체 선검증, 실제 파일 오류 때 완료 원본/완료 판만 확정·뒤 대상 파일 보존·공유 링크 선폐기·남은 대상 처리, 폐기 DB 실패의 파일 보존·결과 DB 실패 뒤 재처리와 감사 기록 중복 방지를 확인한다. 기존 진행 중 thread·완료 보관 참조 보호 시험도 유지한다.
- 새 host 시험의 첫 전체 실행은 다중 프로젝트 fixture를 신규 생성 API로 준비해 단일 프로젝트 정책에 막혔다(671개 통과·새 시험 하나 실패). fixture만 옛 다중 프로젝트 폴더 형태로 직접 준비한다. 제품 정책·기대값을 바꾸지 않는다.
- `AAA_E2E_WORKERS=10 NODE_PATH=<작업 폴더의 Playwright 1.64.1> nice -n 15 bash scripts/ci-local.sh`로 전체 검증한다. E2E113·114의 기존 실제 500개/601개·네 화면·부분 실패/남은 101개 요청·최종 DB/파일/화면 단언을 유지한다. E2E113은 삭제의 성공 실험실 로그도 회수하고 500개 HTTP 응답이 10초 미만인지 검사한다. 기존 응답 20초·단계 시한·재시도 정책을 늘리지 않는다. 브라우저는 내부 공용 캐시만 사용한다.
- 최종 전체 gate는 359초에 web·fmt·build·strict clippy·test·e2e·public 모두 통과했다. 웹 유닛 581개, Rust 1,351개(기존 ignored 6), Python 56개(기존 skip 2), 웹 E2E 114파일/140작업(10 worker, 297.6초)이 통과했다. E2E113/114는 각각 108.8/124.1초였다. 500개 삭제 HTTP는 Chromium 1280/390px 1,013/2,087ms, WebKit 1280/390px 7,738/4,856ms로 모두 10초 미만이었다. 최댓값의 링크 폐기/결과 확정 DB는 0.669/10.413ms, 파일은 7,228.357ms, 조회/보호는 7.723ms, library 잠금 대기는 486.106ms였다. 같은 회차의 관측값이며 운영 성능 보장은 아니다.
- 참고: [SQLite transaction 비용](https://www.sqlite.org/faq.html#q19)([public domain](https://www.sqlite.org/copyright.html)), [rusqlite Transaction](https://docs.rs/rusqlite/0.37.0/rusqlite/struct.Transaction.html)([MIT](https://github.com/rusqlite/rusqlite/blob/master/LICENSE)). 명시적 commit과 실패 rollback을 따르며 외부 코드·의존성을 추가하지 않는다.
## E2E109 첫 앞 기록 요청의 준비 사건(t-1044)

- t-1043의 초기 단계 실패는 `historyApplied=0`, `historyMore=1`, `historyWaiting=0`, `viewTop=viewBase=1992`였고 송신은 resize 한 건뿐이었다. 당시 스크롤 전 DOM 치수는 없어 세부 순서를 소급 확정하지 않는다. 수정 전 E2E109는 worker 10의 일반 부하 20회와 Chromium CPU 4배 감속 20회에서 모두 통과했다. 통과 반복을 원인 확인으로 대신하지 않는다.
- xterm 5.5.0의 실제 `Viewport.syncScrollArea`·예약 frame·`_innerRefresh`·`_handleScroll`을 실행하는 회귀 시험은 본문 표지가 있어도 아직 DOM 스크롤 영역은 0인 순서를 제어한다. 기존 fixture처럼 먼저 `scrollTop=0`을 쓰면 사건이 없고, 늦은 frame은 맨 아래로 동기화해 이동량이 0이다. 새 helper는 준비 전 쓰기 0회, frame 반영과 크기 맞춤 뒤 쓰기 정확히 1회, 실제 xterm의 맨 위 이동을 확인한다. 같은 패치 소스의 별도 headless Chromium·WebKit 실험에서도 본문 표시/스크롤 높이 0 → 초기 0 쓰기 → 맨 아래 유지가 재현됐다.
- E2E109는 `data-fit=same`·맞추기 대기 없음·기록 있음과 실제 행 높이/스크롤 높이/버퍼의 맨 아래 위치가 일치할 때만 한 번 맨 위로 옮긴다. 본문 표시만으로 DOM 입력 준비를 간주하지 않는다. 기다림은 기존 Playwright rAF 조건 대기이고, 준비 전에는 입력하지 않는다. 고정 sleep·입력 재시도·시한 증가·기대값 완화는 없다. 제품·프로토콜·E2E 번호를 바꾸지 않는다.
- 숨김 앞 기록 검사도 같은 준비를 쓰고, 숨기기 직전의 적용 횟수보다 새 응답 적용 횟수가 증가해야 다음 단계로 간다. 이전 응답의 `historyApplied>0`만으로 통과하지 않는다. 기존 두 브라우저·24회 복귀·동일 xterm/연결/snapshot·첫 SGR 휠과 실제 PTY 정확히 한 번·만료·Reader 단언은 유지한다. 실패 자료에는 준비/요청 단계, 이동 전후 치수, 기록 요청 목록과 최근 scroll 사건을 더 남긴다.
- 실행: `node --test web/test/xterm-source.test.mjs`, `node web/e2e/web.e2e.mjs --spec 109 --repeat 20 --workers 10`, `AAA_E2E_THROTTLE=4 node web/e2e/web.e2e.mjs --spec 109 --repeat 20 --workers 10`, `AAA_E2E_WORKERS=10 nice -n 15 bash scripts/ci-local.sh`. Playwright npm 1.64.1만 작업 폴더에 두고 내부 공용 브라우저 캐시를 쓴다.
- OSS 후보는 [xterm.js 5.5.0 Viewport](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))와 [Playwright 조건 대기](https://playwright.dev/docs/api/class-page#page-wait-for-function)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))다. 비동기 viewport 반영과 DOM 준비 조건을 참고하며 외부 코드·의존성을 추가하지 않는다.

## E2E114 자동화 fixture의 외부 writer 경합(t-1038 후속)

- 같은 `231d7e3b` CI의 macOS web-e2e에서 E2E114가 60.5초 뒤 `대량 자동화 입력` 단계의 `database is locked`로 실패했다. `bulk-automation.mjs`는 실행 중 host의 DB에 Node `DatabaseSync`로 자동화 meta를 직접 썼으며, 자리 fixture에 적용했던 host 종료·transaction 준비가 없었다.
- 자리 fixture의 종료·준비·재연결 절차를 `bulk-fixture.mjs`의 `seedHost`로 공유한다. host 종료 사건을 기다린 뒤 `BEGIN IMMEDIATE` 안에서 버리기/계속 보내기 두 규칙과 상태를 함께 준비하고 DB를 닫는다. 다시 띄운 host의 `웹 listen`·`agent 연결` 사건 뒤 실제 UI를 실행한다. 결과 확인 연결은 읽기 전용이다. 준비 실패에는 DB 경로·하위 원인을 남기며 미완성 fixture로 재시작하지 않는다.
- `bulk-fixture.test.mjs`는 실제 SQLite 쓰기 잠금으로 종료를 기다리지 않는 경로의 `database is locked`를 결정적으로 확인한다. 별도 task의 종료 사건에서 잠금을 풀면 같은 helper의 준비·확정·재연결이 끝나야 한다. callback 오류의 부분 쓰기 rollback·잠금 해제·원인 보존과 재시작 host 조기 종료 전파도 검사한다. 세 시험은 Mac과 Windows Node 24에서 모두 통과했다. 고정 sleep·재시도·busy_timeout·시한 증가는 없다.
- E2E114의 Chromium/WebKit × 1280/390px × 버리기/계속 보내기에서 기존 500개 pending·500개 결과 불명, inflight 제거, 남은 입력 0/500, 요청 정확히 한 번과 창·목록 갱신 단언을 유지한다. 같은 helper의 실제 worktree 1개·잔여 500개 정리와 500개 자동화 변환·원문 보존도 그대로 실행한다.
- Mac 웹 유닛 576개·타입/번들·내부 실행 파일 빌드가 통과했다. `NODE_PATH`는 작업 폴더의 Playwright 1.64.1 패키지로만 지정하고 기존 내부 공용 Chromium/WebKit 캐시를 썼다. E2E40·113·114를 worker 3개로 함께 실행해 세 파일 모두 94.1초에 통과했다. E2E114의 자동화 입력 단계는 15.9초였고 네 조합·두 동작의 실제 요청과 상태 단언을 모두 확인했다. 운영 host·브라우저·시스템 설정은 바꾸지 않았다.
- 참고 후보는 [SQLite WAL](https://www.sqlite.org/wal.html)([public domain](https://www.sqlite.org/copyright.html))과 [Playwright web server 수명](https://playwright.dev/docs/test-webserver)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))이다. SQLite의 단일 writer 제약과 서버 종료·준비 사건에 따른 fixture 수명 분리를 따른다. 외부 코드·의존성은 추가하지 않는다.

## Windows sweep fixture의 종료 순서(t-1038)

- `231d7e3b`의 `lifecycle::sweep_tests`를 Windows 격리 worktree에서 실행하면 재개 보존·저장 실패 보존 시험이 결과 단언 뒤 `Lab::drop`의 폴더 삭제에서 공유 위반(OS 오류 32)으로 실패한다. 구형 agent·probe 실패 시험도 같은 정리 오류가 겹쳐 unwind 중 두 번째 panic으로 시험 실행 파일이 중단됐다. 같은 기준의 macOS에서는 4개 모두 통과했다.
- fixture는 `close().await`로 가짜 agent를 취소하고 실제 종료를 기다린 뒤 Host·SQLite 연결을 해제하고 임시 폴더를 삭제한다. 저장 실패를 주입한 별도 SQLite 연결도 먼저 닫는다. 삭제 실패에는 경로와 하위 OS 오류를 남기며 정상 정리 실패는 계속 시험 실패다.
- 기존 601개 묶음/개별 정리·dry-run·구형 agent·probe 오류·확인 중 재개·DB 저장 실패의 결과와 원인 보존 단언을 그대로 실행한다. 제품 코드·시간 상한·기대값·OS별 시험 범위를 바꾸지 않으며 sleep·재시도를 추가하지 않는다. Windows는 `cargo test --profile verify --locked -p aaa-host --lib`, macOS는 `ci-local.sh`의 관련 fmt·clippy·test·public 단계로 검증한다.
- 최종 Windows host lib는 648개 통과·기존 ignored 3개다. macOS는 Rust 1,342개 통과·기존 ignored 6개, Python 56개 중 기존 skip 2개, strict all-targets clippy·fmt·공개 검사가 통과했다. Windows 첫 전체 검사의 별도 실패 4개는 전역 `autocrlf=true`가 바꾼 스키마·예제 CRLF 입력 때문이었다. 격리 worktree의 해당 세 파일만 CI와 같은 LF로 재생성하고 바이트를 확인했다. 시스템 설정·스키마·예제·시험 기대값은 바꾸지 않았다.
- OSS 후보는 [Tokio JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))과 [tempfile TempDir](https://docs.rs/tempfile/latest/tempfile/struct.TempDir.html)([MIT](https://github.com/Stebalien/tempfile/blob/master/LICENSE-MIT))이다. 전자의 취소 후 join으로 자원 해제를 확인하는 방식과 후자의 디렉터리 삭제 전 파일 handle 해제 원칙을 따른다. 외부 코드·의존성은 추가하지 않는다.

## 보관 화면 도입 뒤 착지 회귀(t-1010 후속)

- t-1012 착지의 E2E11·25·29는 보관 xterm도 DOM에 남는데 전역 `.xterm-rows`·`.xterm-helper-textarea`·`.term-scroll`을 찾았다. E2E11의 공통 화면 읽기는 strict 오류를 기다림에서 삼켰고, 진단은 첫 보관 pane(chat1)을 읽었다. E2E29는 숨긴 hold1에서 hold2의 prompt를 찾았다. 제품은 활성 `.term`만 `hidden`·`inert`를 해제하고 그 안의 term-box에만 `data-testid="terminal"`을 부여한다. 화면 읽기·IME·Ctrl+Enter는 이 표지 아래로, 표지 바깥의 스크롤 칸·입력창 측정은 `.term:not([hidden])` 아래로 범위를 좁힌다. 첫 요소 선택이나 force click으로 모호성을 숨기지 않는다. 실패 진단에는 활성 pane과 보관 pane/숨김 여부를 함께 남긴다.
- E2E37의 기존 복귀 gate는 매번 새 WebSocket의 제어 응답이 온다고 가정했다. 먼저 보관 복귀의 같은 DOM·추가 연결 없음·읽던 줄 1px 이내를 검사한다. 이어 숨긴 대상 연결을 실제로 닫고 readonly 사건 뒤 돌아와 새 연결 한 번과 snapshot 도착을 확인한다. 그 응답은 기존처럼 붙잡아 연결 안내 전후 읽던 줄의 1px 계약을 검사한다. Chromium/WebKit × 1280/390/820px × Claude/Codex 12조합을 모두 유지한다.
- t-1012 원본 로그와 pane 상태를 대조하고, 수정 후 관련 15작업에서 E2E11의 첨부 칸 측정도 같은 전역 선택자 문제임을 확인해 함께 고쳤다. 시간 상한·재시도 정책·기존 입력/위치 기대값은 바꾸지 않는다. 보관 연결의 실제 수명·숨김 입력 차단과 E2E109 휠 정확히 한 번은 기존 강화된 검사를 그대로 실행한다.
- 최신 `origin/main` `b44e35bd` 기준 `ci-local.sh --only web,e2e,public`에서 웹 유닛 559개·빌드, 웹 E2E 111파일/137작업(worker 10, 243.3초), 공개 검사가 통과했다. 본 후속은 시험·문서만 바꾸며 제품 코드는 기존 보관 연결 후보 그대로다.
- 참고: [Playwright locator 범위](https://playwright.dev/docs/locators), [WebSocketRoute 연결/종료](https://playwright.dev/docs/api/class-websocketroute)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)). 명시적인 활성 표지와 실제 연결 종료 사건을 이용했으며 외부 코드·의존성을 추가하지 않았다.

## 보관 대화의 연결 수명(t-1010)

- 보관 DOM·xterm·pane WS는 같은 수명을 쓴다. 일반 3개와 coordinator 1개를 각각 LRU로 제한하며 숨긴 지 5분이 지나면 함께 제거한다. coordinator도 기존 대화 본문 8MiB 공통 예산에서 예외가 아니다. 일반 셸을 보는 동안은 그 연결도 일반 세 자리 안에 둔다.
- 숨긴 pane은 크기를 release하고 입력·resize·앞 터미널 기록 요청을 보내지 않는다. 압축 출력은 계속 파싱하고 ACK한다. 기존 서버의 흐름 제어는 ACK 중단 시 연결을 닫으므로 일시 정지 프로토콜을 추가하지 않았다. 숨은 xterm은 그리기를 멈추고, ChatPanel의 HTTP 갱신도 멈춘다. 앞 터미널 기록을 붙이는 도중 숨기면 파싱 완료 뒤 render 대기를 풀어 출력이 쌓이지 않게 한다.
- `retainedChats.test.mjs`는 일반/특별 자리, 두 coordinator의 교체, 개별 5분 경계, 공통 용량 퇴거, 일반 셸 자리와 기존 삭제·세션 교체를 검사한다. E2E104는 Chromium/WebKit × 1280/390px에서 실제 최대 네 소켓·일반 LRU·만료 후 소켓 종료·숨김 송신 없음과 기존 본문/초안/예산/리스너 검사를 수행한다.
- E2E84는 제어한 시계로 첫 연결/재연결의 입력창 안내를 499ms까지 숨기고 500ms부터 표시하는지 확인한다. 빠른 연결에는 평소 placeholder를 유지하며 보내기 가능 여부는 기존 connected 조건 그대로다.
- E2E109는 실제 coordinator를 일반 이동/Ctrl+Tab으로 각각 12회 왕복한다. 보관 복귀는 새 연결 없이 살아 있는 xterm에 첫 휠을 보낸다. 송신 정확히 1프레임과 실제 PTY의 SGR 휠 정확히 1개를 함께 확인하고, 후속 입력 도착까지 중복이 없는지 확인한다. 5분 만료 뒤에는 새 snapshot 전 송신 0개, 준비 뒤 첫 휠 1개를 검사한다. 기존 20초 상한은 유지한다. 앞 기록 응답을 보류한 채 숨긴 뒤 응답을 풀어, render 없이 파싱 완료·출력/ACK 진행과 크기 점유 해제를 확인한다. 만료는 `clock.fastForward`로 타이머 사건만 실행하며 5분치 애니메이션 frame을 합성하지 않는다.
- t-1009 원문은 b44e35bd 실패를 원인 미확정으로 분류했다. 시간 초과만으로 미송신/중복을 구별하거나 공용 실패 화면을 대상 pane으로 단정하지 않는다. E2E109의 실패 자료에는 대상 pane·실제 활성 pane·이동 목적지/방식/회차·송신 목록·PTY 수신 수를 남긴다. 복귀 때 새 xterm을 만들던 경로와 그 휠 큐를 제거했으며, 이는 과거 CI 원인을 소급 확정한 것이 아니다.
- 참고: [herdr-web-ui PaneTerminal](https://github.com/devswha/herdr-web-ui/blob/main/src/components/PaneTerminal.tsx)([MIT](https://github.com/devswha/herdr-web-ui/blob/main/LICENSE))의 보관/표시/크기 권한 분리, [VS Code TerminalInstance](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/terminalInstance.ts)([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt))의 표시 수명과 인스턴스 수명 분리, [Chrome Page Lifecycle](https://developer.chrome.com/docs/web-platform/page-lifecycle-api)(문서 CC-BY-4.0, 예제 Apache-2.0)의 숨김/폐기 구분을 대조했다. 외부 코드·의존성은 추가하지 않았다. 브라우저가 탭 자체를 폐기하는 경우는 웹앱이 연결을 보장할 수 없으며 기존 재연결 경로를 따른다.

- 최종 검증: 웹 유닛 559개·타입/번들·host 빌드·의존 경계 2개·공개 검사, 관련 E2E12파일/27작업(worker 4, 128.3초)과 CPU 4배 감속 Chromium E2E109·동반 E2E93(worker 2, 두 작업 30.0초)을 통과했다. E2E109의 Chromium 1280·WebKit 390에서 실제 PTY 휠 1회와 만료/숨김 앞 기록을 확인했다. E2E103의 기존 보관 계측도 별도 통과했다.
- 첫 검증의 E2E109 WebKit은 12회 실제 휠 검사 뒤 `clock.runFor(5분)`에서 단계 120초 상한에 걸렸다. 만료 검사에 필요 없는 반복 frame을 `fastForward`로 생략하고 frame 관측기를 복귀 구간에만 켰다. 상한·정확히 한 번 단언은 그대로이며 같은 실패를 재시도해 통과시키지 않았다. E2E30은 실제 ACK 보류 뒤 숨김 연결을 명시적으로 끊어 기존 초안 복구/중복 제출 방지 계약을 유지한다.

## 실기 영숫자 지연과 PTY 순차 가림(t-1002)

- iPhone 실기에서 `status aaa123!`의 ASCII는 composition 없이 각 keydown에서 즉시 send했다. 기존 `redact::Stream`에 영숫자 접두부를 가진 합성 인증을 등록하면 `status→statu`, 숫자 한 글자 보류, 구두점에서 해제가 재현된다. 실제 인증을 시험·보고에 쓰지 않는다.
- q-1285 1A는 PTY 앞 최대 7바이트 선표시를 허용했다. `LiveStream`은 각 push의 모든 바이트를 바로 반환하고 원문 및 CSI·CR/LF를 뺀 8바이트 창만 유지한다. 겹치는 조각의 8번째 이후 바이트는 가린다. 일반 `Stream`/`bytes`의 완전 가림은 유지한다.
- `cargo test --locked -p aaa-common --lib redact::`는 실기 send 순서의 각 글자 즉시 반환, 모든 청크 경계·한 바이트 청크, ANSI·개행 사이 인증 조각, 겹치는 조각, 기존 완전 가림을 검사한다.
- `cargo test --locked -p aaa-session --lib output::tests`는 실제 query scanner→순차 필터→VT에서 각 글자가 다음 입력 전 화면에 나타나는지, 한글·제어문자·붙여넣기 표지, DSR·색·resize·EOF를 확인한다. 기존 접두부 보류/전체 별표 기대는 승인된 PTY 계약에 맞춰 앞 7바이트+가림으로 바꾸되 8바이트 원문 비노출 검사를 유지한다.
- `cargo test --locked -p aaa --test m13 accounts::authenticated_terminal_echo_does_not_wait_for_the_next_ascii_key -- --exact`는 합성 인증을 가진 실제 host/agent/session/PTY에 실기 send와 같은 글자를 한 번에 하나씩 넣는다. 그 글자의 화면 반영을 확인하기 전에는 다음 글자를 보내지 않는다. 출력 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다.
- 기존 `accounts::account_credentials_parallel_refresh_and_all_records_are_redacted`도 실행해 로컬·TLS 원격 계정/갱신/출력/기록의 원문·8바이트 조각 비노출을 확인한다. 브라우저 입력 코드는 바꾸지 않는다. 실제 iPhone·운영 Claude 확인은 새 session-host 적용 후 사용자가 한다.

## 할 일 제목·칩 시작점 정렬(t-1021)

- E2E107은 손잡이와 완료 원 사이가 16px 이내이고, 첫 칩과 제목 글의 시작점이 하위 들여쓰기를 포함해 1px 이내인지 검사한다. 기존 열 정렬·겹침·잘림·휴대폰 제목 폭·행 높이·첨부·메모·선택·폭 변경 뒤 표지 한 개 검사를 유지하며, Chromium 1280/2000/390/320px와 WebKit 1366/1024/390/320px를 쓴다. 화면은 실제 격리 host·agent에서 headless로 찍고 DOM·글꼴 준비 사건을 기다린다.
- 원인은 `9a506f8b`(main 반영 `ae97a6ea`)의 고정 `5em` 단계 열과 별도 칩 들여쓰기다. 그 부모에는 빈 단계 열이 없다. 최근 ID 자릿수·머신 이름 변경에서 생긴 회귀가 아니다. 기존 판 대조에서 데스크톱 손잡이/완료 원 사이 81px, 제목/칩 시작점 약 150px 차이로 새 검사가 실패한다.
- 단계 표지를 모든 폭에서 메타정보 줄에 두고 상태·ID 열 너비와 하위 들여쓰기를 칩에도 공유한다. 둘째 줄은 메뉴 아래 공간까지 쓴다. 첫 수정안에서 320px의 칩 줄이 불필요하게 접혀 기존 76px 행 높이 검사가 실패했으며, 메뉴 아래의 빈 공간을 사용하도록 고쳤다. 시한·기대값·검사 범위는 완화하지 않는다.
- 실행: `npm test --prefix web`, `npm run build --prefix web`, `cargo test --locked -p aaa --test deps`, `node web/e2e/web.e2e.mjs --spec 43,46,47,52,54,67,70,75,76,107 --workers 4`. Playwright npm 1.64.1만 작업 폴더에 두고 기존 공용 브라우저 캐시를 쓴다. 브라우저·시스템 설정은 변경하지 않는다.
- 참고: [herdr-web-ui Sidebar.css](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/Sidebar.css)의 공통 행 열·제목/보조 정보 배치와 [herdr-projects popup.rs](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/popup.rs)의 제목·담당·thread·메모 순서를 대조했다. 두 저장소의 고정 판 LICENSE는 MIT이며 구성만 참고하고 외부 코드·의존성을 추가하지 않았다.

# 시험 쓰는 법

## E2E105 대화 focus·입력 위치 복원(t-1049)

- 기존 E2E105에 Chromium·WebKit × 1280/390px 대화 전환을 추가한다. 첫 진입의 메시지 입력 기본 focus, 서로 다른 초안·중간 커서·역방향 선택의 Ctrl+Tab 왕복, 복귀 직후 실제 삽입/선택 교체, 선택 창에 focus가 옮겨진 뒤 Esc 취소, 본문 복사 버튼 복귀, 사라진 찾기 입력의 기본 입력 복귀, 일반 대화 3개 보관을 넘긴 뒤 초안·선택 복원을 검사한다. 전환 키·초안의 PTY 전송 0회와 숨은 pane의 inert도 확인한다.
- 390px 터치 조건에서는 첫 진입과 실제 목록을 통한 왕복 중 composer/xterm focus 사건이 없어야 한다. 화면 자판을 열 원인이 되는 자동 focus 경계를 검사하는 headless 시험이며 실제 iPhone 화면 자판 검증으로 세지 않는다. 기존 PWA 주입·일반 탭·설정·MRU·앱 화면 조합도 유지한다.
- 첫 부분 시험은 커서 6이 글 끝 12로 바뀌었다. 사건 기록에서 hidden 반영 뒤 focusout(선택 6)이 먼저 오고 select는 다음 화면이 열린 뒤 도착했다. 비활성이라는 이유로 focusout까지 버리던 경로를 마지막 대상의 이탈 사건에 한해 받도록 고친다. 고정 sleep·시한 연장·기대값 완화 없이 같은 중간 커서 단언으로 확인한다. 긴 입력창은 display:none 뒤 scrollTop이 0으로 보이므로 숨기기 전 키/scroll 사건의 값을 유지한다. 같은 37px 왕복 단언으로 확인하며 실패 진단은 focus/select/input/scroll 사건·대상 pane·숨김·선택 범위를 남긴다.
- WebKit은 focusin에서 임시 커서 0을 보이고 같은 범위를 다시 설정해도 select 사건이 없을 수 있다. 복원 중 사건은 기억을 덮지 않고 복원 완료 위치를 직접 확정한다. 선택 창의 취소도 명시적으로 범위·방향·입력 스크롤을 복원한다. 패널 복귀 직후와 이어지는 두 대화 왕복, 실제 focus가 선택 창으로 옮겨진 뒤 Esc 취소를 각각 단언한다.
- 실행은 `NODE_PATH=<작업 폴더의 Playwright 1.64.1> node web/e2e/web.e2e.mjs --spec 105 --workers 1`과 전체 ci-local이다. 내부 공용 브라우저 캐시를 쓰며 전체 gate는 머신의 차례를 받은 뒤 실행한다.
- 참고 후보는 [VS Code FocusTracker](https://github.com/microsoft/vscode/blob/main/src/vs/base/browser/dom.ts)([MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt))와 [ReactInputSelection](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/client/ReactInputSelection.js)([MIT](https://github.com/facebook/react/blob/main/LICENSE))다. 영역별 focus 추적과 살아 있는 대상의 선택 복원·스크롤 보존을 참고한다. 외부 코드·의존성 추가는 없다.

## coordinator 복귀의 연결 준비·본문 보존(t-0998)

- 57회차 후속: 기존 후보의 E2E93 1280px 재개 복귀에서 첫 휠이 사라졌다. 다른 E2E와 worker 6개로 병렬 실행해 9·13·15단계가 실패했고, 관측 실행은 snapshot 수신 8731ms → 파싱 ACK 8732ms → wheel 8737.4ms를 보였다. 당시 readonly=false인데 실제 xterm은 visibility:hidden, 사본은 pointer-events:none이어서 event.target은 term-box였고 입력 프로그램에는 두 번째 휠이 도착하지 않았다. 부하 시간 초과가 아닌 입력 대상 제거 회귀다.
- opacity만 고친 대조에서도 snapshot보다 약 5ms 먼저 온 휠은 새 xterm의 아직 빈 모드에서 사라졌다. 표시 사본이 있는 복귀 동안만 휠을 최대 64개 보관하고, snapshot 파싱·그리기 뒤 기존 xterm 휠 경로로 한 번 전달한다. 화면 이탈·실제 readonly에는 버린다. 텍스트·클릭 입력을 자동 보관/재전송하지 않는다. E2E109는 snapshot gate 중 송신 0회→해제 뒤 정확히 1회, 중간 이탈 뒤 0회를 검증한다. opacity 단독판에서 이 단언도 RED다.
- 첫 render도 스크롤 준비 완료와 같지 않다. 재전달 관측에서 권한·VT200/SGR·cell 치수는 정상인데 Viewport 줄 높이가 0이었다. RIS 뒤 두 frame에 걸친 치수 동기화 전에는 기존 NaN 방지 패치가 픽셀 휠을 버린다. 새 snapshot의 render에서 `syncScrollArea(true)`로 치수를 동기화한 뒤 사본 제거·휠 전달을 한다. 고정 frame/시간 대기나 NaN 보호 해제는 없다.
- WebKit의 재전달은 치수 동기화 후에도 옛 연결의 `adopted=true`가 남은 순간에는 막혔다. 새 front/state가 React 상태에 반영되어 화면 점유가 풀리는 사건에서도 보관한 휠을 전달한다. 그 전에는 꺼내 버리지 않으며, 이탈 시에는 그대로 폐기한다.
- 새 xterm은 opacity=0으로 글자만 가려 휠·focus 대상에서 빠지지 않게 한다. viewport와 앞 기록 덮개도 사본이 섞인 바깥 칸이 아닌 해당 xterm 인스턴스에서 찾는다. E2E109에 실제 보이는 영역의 elementFromPoint가 새 xterm 안인지 단언을 더해 구판의 RED를 확인했다. E2E93에는 휠 대상·readonly·사본/실제 visibility·focus·WS 시각을 실패 진단에 추가하며, 기존 입력·완료 조건·20초 상한은 그대로 둔다.

- 후속 최종 검증: worker 10개로 E2E93 전 16단계·109·105와 E2E02·03·07·13·43·47·52·84·104를 한 번 병렬 실행해 12개 파일·27개 작업 전부 통과했다. 웹 유닛 548개·타입/번들/host 빌드·공개 검사도 통과했다. 원래 시한·기대값은 유지했으며, 실패한 수정안은 원인 관측 후 코드 변경으로만 진행했다.

- E2E109는 실제 host·agent·coordinator와 2,400행 합성 Claude 기록을 준비한다. WebSocket open은 통과시키고 실제 수신 메시지만 gate에 보류해 snapshot 전을 검사한다. thread·할 일·보고서에서 일반 이동/standalone Ctrl+Tab 각각 두 번씩 돌아오며, MutationObserver와 매 frame에서 거짓 「읽기 전용」이 없는지, 이전 터미널 본문이 첫 화면부터 남는지 확인한다. gate를 풀면 표시 사본이 없어지고 실제 입력이 PTY 화면에 나타나야 한다. Chromium 1280px·WebKit 390px, 합계 24회다. OS가 예약한 Ctrl+Tab의 네이티브 전달 시험은 아니다.
- 수정 전 Chromium의 12회 모두 거짓 띠와 빈 본문이 재현됐다. 연결 완료를 socket open에서 snapshot 수신으로 옮기고, 이탈 시 xterm을 정리하면서 표시 전용 DOM 사본만 남긴다. active 조건으로 term-stack도 사라지므로 복귀 layout effect에서 새 칸으로 옮기고, 새 snapshot의 파싱·render 완료 뒤 사본을 제거한다. 소켓·xterm을 숨은 동안 살려 두거나 기록을 별도 저장하지 않는다.
- E2E109의 Reader 복귀는 「보기 권한」 안내와 입력 송신 0회를 확인한다. 기존 E2E02는 다른 controller가 크기를 점유해도 동시 입력이 가능한 제품 정책을 유지하고, E2E13은 실제 agent 끊김/복구·host 재시작의 안내를 검사한다. E2E84의 499/500ms 연결 안내, E2E104의 보관·숨김 송신·DOM/관측기 수명, E2E105의 전환 키 경계도 유지한다. 고정 sleep·시한 연장·실패 재시도는 없다.
- 검증: 웹 유닛 548개·타입 검사·번들/host 빌드, E2E02·07·09·13·30(chat-reading/thread-preparation)·84·97·103·104·105·109 총 12개 파일과 공개 검사를 통과했다. 새 fixture의 입력 방식/단추 선택자를 실제 UI에 맞추고, Reader 휴대폰 기본 대화 보기에서는 터미널로 전환한 뒤 입력 차단을 검사했다. 제품 시한·기대값은 바꾸지 않았다. 운영 배포·실기 확인은 이 검증에 포함하지 않는다.
- 참고 후보는 [herdr-web-ui의 PaneTerminal](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/PaneTerminal.tsx)([MIT](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE))와 [herdr의 wire protocol](https://github.com/herdrdev/herdr/blob/v0.9.5/src/protocol/wire.rs)([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE))이다. 전자의 presented/connected/inputReady/outputReady 분리와 후자의 연결 handshake·pane 표시 메시지 구분을 대조했다. 기존 aaa snapshot·권한 경로에만 적용하며 코드 복사·외부 의존성·API 변경은 없다.

## 하단 Claude의 등록 계정 자료원(t-0999)

- 운영에서 기본 키체인의 끝난 OAuth와 정상 동작하는 등록 계정의 상태줄은 서로 다른 인증이었다. 하단은 등록 Claude 계정이 있으면 기존 A2 수집의 계정·로그인 세대별 관측을 쓰고, 없으면 기존 기본 홈 수집을 유지한다. Codex와 자동화의 로컬 사용량 캐시는 그대로다.
- `cargo test -p aaa-host --lib usage::tests`는 기본 OAuth 만료와 등록 계정의 5시간 98%·주간 6%가 공존하는 실제 Host·DB를 검사한다. 재등록 뒤 이전 세대 배제, 다른 계정·모델 몫·충돌 배제, 옛 관측 시각 보존, 403과 만료 구별을 확인한다. 인증 갱신·실제 계정·외부 구독 요청은 없다.
- E2E65는 가짜 Claude가 실제 상태줄 hook으로 보낸 98%·6%를 agent→host→하단 칩·팝업까지 확인한다. 기본 홈 OAuth는 만료시킨다. Chromium 1280·390px에서 같은 계정 한 개와 Codex의 기존 27%를 확인하며, 관측 전에는 「관측 대기」를 보인다. 기존 계정 카드·401/429·잠김·단절·세대 교체·원문 비노출 검사를 유지한다. DOM·상태줄 사건으로 기다리며 sleep·재시도·시한 확대는 없다.
- 웹 유닛은 수집 실패 사유의 빈 값·재로그인 단정 방지를 확인한다. proto TypeScript 생성 계약, 웹 유닛·타입·빌드, host 전체 유닛, 의존 경계·clippy·공개 검사를 수행한다. 운영 배포와 실기 UI 확인은 포함하지 않는다.
- 먼저 [CodexBar의 Claude 자료원](https://github.com/steipete/CodexBar/blob/main/docs/claude.md)([MIT](https://github.com/steipete/CodexBar/blob/main/LICENSE))과 [OpenUsage의 인증 자료원](https://github.com/robinebers/openusage/blob/main/docs/providers/claude.md)([MIT](https://github.com/robinebers/openusage/blob/main/LICENSE))을 대조했다. 모델 실행 인증과 사용량 조회 인증의 차이·계정 귀속을 참고하고, 구현은 기존 A2 상태줄 수집을 재사용한다. 외부 코드·의존성은 추가하지 않는다.

## 휴대폰 터미널 영어의 마지막 글자 지연(t-1001)

- E2E09의 「휴대폰 영어 조합」은 WebKit·Chromium의 iPhone UA·390px에서 다음 키가 이전 composition을 끝내는 순서를 재생한다. 마지막 `s`의 compositionend나 다음 키를 주기 전에 실제 host→agent→PTY에 `status`가 모두 도착해야 한다. 양 엔진은 공용 브라우저 캐시와 저장소의 Chromium launcher를 쓴다.
- 구판 소스 대조는 `statu`, 수정판은 `status`다. 구판 WebKit E2E도 마지막 글자 송신 조건에서 실패한다. 원인은 이 사건 순서에서 xterm이 인쇄 가능한 영어도 IME 조합으로 끝까지 보류하는 것이다. 이번 영어 현상의 실제 iPhone 사건 기록은 제공되지 않았으므로 기기의 정확한 사건 순서까지 확인한 결과는 아니다. 배포 뒤 iPhone 확인은 사용자가 한다.
- 자동 수정 `status→state`의 정확한 DEL/새 꼬리 bytes, Space·Backspace·Enter, 실제 한글 composition·iPhone의 composition 없는 한글 replacement, 붙여넣기 전 한글 확정, composition 없는 영어 키를 같은 실제 PTY에서 검사한다. 가로챈 입력만 보고 통과시키지 않는다. 시험 프로그램이 실제 stdin 원본 bytes를 파일로 보존한다. 기존 paste fixture의 화면 로그는 JSON.stringify가 DEL을 이스케이프하지 않아 터미널에서 삭제 byte를 잃으므로 삭제 검증에 쓰지 않는다. 붙여넣기의 bracketed paste 표지도 정확히 비교한다. WebSocket 송신·PTY 수신·DOM 준비 사건으로 끝내며 고정 sleep·재시도·시한 확대는 추가하지 않는다.
- `web/test/mobile-composition.test.mjs`는 실제 배포 패치가 적용된 upstream helper로 구판/수정판, 한 단어 교정·삭제, 한글 보류·영문에서 한글 전환, 같은 task의 한글 확정→영문 순서, keydown 229 fallback 중복 방지, 데스크톱·화면 읽기 모드를 확인한다. 기존 E2E03·08·09·10으로 Ctrl+Enter·대기 말·입력줄·화면 키보드 회귀를 함께 검사한다.
- `Term.tsx`의 onData→fromTerminal→deliver는 Claude·Codex·셸이 공유한다. 별도 `TerminalInput`의 입력줄 조합 확정/전송과 Ctrl+Enter 판정은 바꾸지 않는다. 프로토콜·backend·E2E 번호도 바꾸지 않는다. 참고 후보·라이선스·교정 범위는 [xterm 패치 설명](../web/patches/README.md)에 적었다.


## E2E93 터치 뒤 반대 방향 관성(t-0996)

- t-0995의 Chromium 390px·Claude 직접 실행 실패는 터치 중 0→197px, 손을 뗀 뒤 197→0px의 native scroll이다. 해당 구간의 JS 위치 쓰기와 안내 높이 변화는 없었다. t-0851의 다른 history 위치 누수, t-0886의 1px 앵커 오분류, t-0905의 프로그램 reset 뒤 ±1px, t-0927·t-0932의 구독/종료 경합과 구별한다. 원본에는 touchmove 시각·compositor trace가 없어 당시 속도 표본까지 소급 확정하지 않는다.
- 앱·React·기록 요청·위치 복원이 없는 최소 HTML에서 같은 8개 CDP move에 불규칙한 timestamp(시작부터 ms: `0,1,2,3,4,5,6,7,39`, 종료 `40`)를 주면 185→0px가 재현된다. Chromium 내부 trace는 손이 위로만 움직였는데 양의 Y fling 속도를 보인다. 실제 E2E93도 이 시각 대조에서 다른 E2E와 병렬로 3/3회 원래의 `scrollTop>0` 30초 대기를 실패했다. 이는 입력 시각을 제어한 원인 대조이며 원본 부하가 같은 표본을 만들었다는 재현은 아니다. 시각을 고정하지 않은 수정 전 E2E93 3회와 최소 HTML 30회는 통과했다.
- 직접 실행의 Chromium 휴대폰 입력을 기존 맨 위 복귀와 같은 `Input.synthesizeScrollGesture`의 `gestureSourceType: touch, preventFling: true`로 보낸다. 브라우저가 터치 이동과 정지를 생성한 뒤 손을 떼며, Node의 개별 move 송신 간격에 관성 속도를 맡기지 않는다. 실제 터치·scrollend 뒤 이동 여부, 맨 위의 정확한 0, 실제 1px·안내 +1px의 읽던 줄, append·복귀·다른 파일 최신 끝 단언은 유지한다. 이 owner는 앱의 읽던 자리 보존을 검사하며 브라우저의 fling 속도 추정 자체를 인수 조건으로 삼지 않는다.
- t-0896의 독립 16단계를 유지한다. 조합마다 새 실험실이며 이번 실패는 합계 120초가 아닌 한 조합의 위치 조건이다. 추가 분할은 이 입력 경합을 없애지 못하므로 하지 않는다. 대신 직접 실행의 안내·1px·입력·맨 위 복귀·append phase를 나누고, 공통 스크롤 진단에 touchmove/cancel의 좌표와 event timestamp를 더한다. 제품 코드·API·번호·기존 단언·20/30/120초 상한은 바꾸지 않는다. 고정 sleep·실패 재시도는 없다.
- 부하 검증은 E2E93의 Claude 직접 실행 Chromium 390px를 `--repeat 12 --workers 3`으로 실행하면서 별도 runner의 E2E43·47·52 전체를 `--repeat 6 --workers 3`으로 함께 실행한다. `--grep`은 E2E93의 독립 조합만 고르며, 선행 준비를 쓰는 동반 spec에는 적용하지 않는다. 전체 E2E93·30·37·103·104와 웹 유닛·타입·빌드·공개 검사도 확인한다. 브라우저는 내부 공용 캐시를 사용하며 iPhone 실기 결과로 해석하지 않는다.
- 참고 후보는 [Chromium의 synthetic smooth move](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/content/common/input/synthetic_smooth_move_gesture.cc)([BSD-3-Clause](https://chromium.googlesource.com/chromium/src/+/main/LICENSE))와 [Playwright의 입력 시험 지침](https://github.com/microsoft/playwright/blob/main/docs/src/input.md)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))이다. 브라우저가 만드는 실제 입력·정지 경계를 따르며 외부 코드·의존성은 추가하지 않는다.

## GitHub Pages 한 줄 설치 입구(t-0987)

- 자동 재게시(t-0990): 같은 계약 시험에 `workflow_run`의 release 완료·성공·동일 저장소·main 문맥과 수동 실행, 배포 gate·두 URL 검증 연결을 단언한다. 실제 workflow의 bash 조건을 추출해 gh 응답 fixture와 실제 jq로 수동·정식판·사전판 tag/flag·초안·과거판·다른 tag·API 503을 검사한다. HTTP fixture로 공개 두 파일의 정상·이전 본문·404를 검사하고, 생성 전 latest가 바뀌면 자산 다운로드·파일 생성 없이 실패하는지 확인한다. 실제 release·tag·Pages 게시는 로컬 시험에서 하지 않는다.
- `python3 -m unittest scripts.test_install_site scripts.test_release`는 로컬 HTTP 서버와 격리 HOME·임시 폴더에서 생성한 입구를 실행한다. macOS·Linux는 실제 POSIX sh와 제어 터미널로 `curl | sh` → `/dev/tty` → `setup` 인자·빈 인자·한글·종료 코드·정리를 검사한다. 네 OS/CPU 자산 선택은 `uname`만 바꾼 fixture로 검사하며 네 CPU의 네이티브 실행 결과로 해석하지 않는다. macOS launcher 동반·기존 설치 파일 보존을 확인한다.
- 체크섬 불일치·누락·중복·manifest 불일치·외부 URL·지원하지 않는 OS/CPU·터미널 없음·다운로드 404·실행 판 불일치 때 setup 호출이 없어야 한다. curl 종료 코드 22도 오류에 남는지 단언한다. 성공만 기다리는 고정 sleep·재시도·시한 확대는 없다. HTTP 응답·PTY EOF·자식 종료가 완료 사건이다. 자식의 제어 터미널 준비는 새 Python 프로세스가 맡아 HTTP thread가 있는 부모의 fork 실행을 피한다.
- Windows는 실제 PE fixture를 rustc로 만들고 PowerShell 5.1·7의 실제 `irm | iex`를 실행한다. 공식 다운로드 URL만 로컬 HTTP로 돌리고 파일 SHA·PE·판·기존 설치 보존·setup 호출·종료 코드를 실제 경계에서 검사한다. 나쁜 SHA는 실행하지 않는다. 생성한 Windows 입구의 bytes가 기존 `release.render_windows_entry` 결과와 같은지도 검사한다. 서비스 설치는 fixture의 범위가 아니다. `ci-local.sh`의 Unix Python 단계와 세 OS `distribution.yml`에서 계속 실행한다.
- macOS와 Lima Ubuntu 24.04 arm64에서 각각 13개 중 12 통과·Windows 전용 1 제외, Windows 실기에서 9 통과·Unix 전용 4 제외다. 로컬 관련 Python 계약 전체는 47개 중 45 통과·기존 Linux 제한 1/Windows 전용 1 제외다. 공개 v0.1.6의 네이티브 macOS arm64·Linux arm64·Windows x86_64 실행 파일도 격리 경로에 실제로 받아 SHA·판 검증 뒤 `setup --help`를 호출했다. 운영 서비스 설치·등록 시험으로 부르지 않는다.
- Windows 첫 준비는 깨진 Scoop Python shim 때문에 실행되지 않았다. 기존 Python 실행 파일을 직접 지정한 뒤 기본 cp1252에서 새 시험의 UTF-8 셸 읽기와 기존 release 시험의 Cargo 읽기가 실패했다. 새 읽기는 UTF-8을 명시하고, 기존 release 시험은 해당 자식의 `PYTHONUTF8=1`로 실행했다. 시스템 PATH·Python·인코딩 설정은 바꾸지 않는다. Windows 서비스·예약 작업·Run 키·사용자 PATH·방화벽 전후 목록을 비교하고 시험 폴더와 Linux VM을 정리한다.
- [Herdr](https://github.com/herdrdev/herdr/blob/master/distribution/install.sh)(Apache-2.0), [Ovis/옛 herdr-web-ui](https://github.com/devswha/ovis/blob/main/install.sh)(MIT), [herdr-projects](https://github.com/eliasstravik/herdr-projects/blob/main/crates/flow/src/install.rs)(MIT)의 플랫폼 선택·검증·터미널 입력·Pages 구성을 참고했다. 상세 라이선스·범위는 [설치 입구 문서](install-pages.md)에 있다. 외부 코드·의존성은 추가하지 않았다.

## E2E90 workspace fixture의 SQLite 쓰기 경합(t-0989)

- 긴 목록 fixture는 실행 중인 host DB를 Node `DatabaseSync`로 열어 직접 INSERT하던 연결을 없앴다. CLI `project set repos.add`로 고유 origin의 실제 Git checkout 24개를 등록하고, 가짜 harness thread의 실제 보고 회수 → agent 신원 수집 → host 원장 저장을 거친다. 기존 thread 두 개와 함께 기본 동시 한도 10개를 넘지 않도록 여섯 개씩 준비한다. dashboard API에 묶음의 모든 이름과 ID가 나타나는 사건을 기다린 뒤 fixture thread를 resolve하여 동시 실행 한도를 비운다. 최종 workspace 25개(기존 두 머신의 같은 origin 1개 + fixture 24개)를 확인한다.
- `repos.add`의 응답은 audit 쓰기 완료만 뜻하며, 앞선 보고 회수의 비동기 지표 저장 완료를 보장하지 않는다. 같은 host의 `collect_workspace_metrics`는 `repo_workspace_id` upsert와 `save_workspace_metrics` transaction으로 `repo_metrics`·`meta`를 쓴다. 별도 Node 연결에는 busy handler가 없어 host writer와 겹치면 SQLite 코드 5(`SQLITE_BUSY`)로 즉시 실패할 수 있다. 다른 E2E는 DB 파일이 격리되어 직접적인 같은 DB writer가 아니다.
- 수정 전 대조는 격리 실험실 DB의 `workspace-collection:%` meta INSERT에 계산만 하는 진단 trigger를 걸어 실제 host 수집 transaction의 잠금 구간을 넓혔다. 보고 변경 뒤 `BEGIN IMMEDIATE`의 코드 5로 host 잠금을 확인하고 기존 `repo_workspaces` INSERT의 `database is locked`·busy timeout 0을 재현했다. 외부 연결이 잠금을 대신 잡지 않았으며 진단 trigger는 제품·정규 시험에 넣지 않는다. 원본 착지 실패 순간의 정확한 SQL을 소급 확정하는 대조는 아니다.
- `NODE_PATH=<격리 Playwright 모듈> node web/e2e/web.e2e.mjs --spec 90,43,47,52 --repeat 3 --workers 6`으로 다른 E2E와 병렬 부하에서 전체 E2E90을 반복한다. 긴 목록 준비 구간에도 다른 보고 회수 시험이 겹치도록 `--spec 90,12-threads-push --repeat 3 --workers 6`도 실행한다. Chromium/WebKit × 1280/820/390/320px × 밝음/어두움, 목록 스크롤·바닥 줄·Reader·URL·조회 오류와 기존 통계 단언을 유지한다. 고정 sleep·재시도·시한 확대·기대값 완화·제품 코드 변경은 없다. 웹 유닛·타입 검사·빌드·공개 검사도 수행한다.
- 최종 두 병렬 실행은 12/12·6/6 작업 통과이며 E2E90은 합계 6/6 통과했다. 묶음 fixture 준비는 약 33~36초, E2E90 전체는 중간 66.3~69.2초였다. 직접 SQL fixture의 약 34초보다 실제 수집 비용이 늘지만, 한 개씩 수집한 초기판의 약 93초보다 짧다. 웹 유닛은 546개 모두 통과했다.
- 먼저 [Playwright fixture 격리](https://github.com/microsoft/playwright/blob/main/docs/src/test-fixtures-js.md)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와 [Node SQLite 연결·timeout](https://github.com/nodejs/node/blob/main/doc/api/sqlite.md)([MIT](https://github.com/nodejs/node/blob/main/LICENSE))을 대조했다. fixture 수명 경계와 연결별 busy handler를 참고하되 기존 제품 CLI·수집 경로를 사용한다. 외부 코드·의존성 추가는 없다.

## 휴대폰 할 일의 빈 열·메모 줄(t-0980)

- E2E107은 기존 WebKit iPad 양 방향·Chromium 데스크톱 두 폭에 WebKit/Chromium 320·390px를 함께 검사한다. 휴대폰 상태 원의 왼쪽 여백 40px 이하, 제목 폭 100px 이상, 행 높이 76px 이하, 메타정보가 없는 행 40px 이하, 메모와 끌기 손잡이의 제목 줄 배치를 확인한다. 기존 열 정렬·교차 면적·잘림·가로 넘침·첨부·상세 열기 검사를 유지한다.
- 수정 전 같은 fixture에서 휴대폰 빈 열·제목 폭 회귀가 실패했다. 휴대폰에서만 빈 단계 열을 없애 실제 단계 표지를 칩 줄로 옮기고, 메모 표시는 제목 단추 안으로 옮긴다. iPad·데스크톱의 공통 열은 유지한다. 화면 크기를 재현한 headless 검사이며 실제 iPhone 실기는 아니다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 107,43,47,52,67,76 --workers 1`과 웹 유닛·타입 검사·빌드·공개 검사를 수행한다. 글꼴·DOM·host 사건으로 기다리며 고정 sleep이나 시한·기대값 완화는 없다.
- 먼저 [Tailwind의 반응형 grid-column](https://tailwindcss.com/docs/grid-column)([MIT](https://github.com/tailwindlabs/tailwindcss/blob/main/LICENSE))과 [Bootstrap의 반응형 grid](https://getbootstrap.com/docs/5.3/layout/grid/)([MIT](https://github.com/twbs/bootstrap/blob/main/LICENSE))를 대조했다. 작은 폭에서 열 구성을 바꾸는 방식을 참고하고 기존 CSS에만 적용한다. 외부 코드·패키지는 추가하지 않는다.
## 머신 판의 짧은 커밋 통일(t-0982)

- 공통 `util::software()`에서 커밋 salt를 12자리로 만든다. Mac 배포 스크립트는 이미 12자리를 전달하며, Windows 전달 빌드처럼 `AAA_BUILD_SALT`에 40자리를 줘도 CLI·setup 안내/영수증·host/agent 보고는 같은 짧은 판을 쓴다. salt 없는 release와 P0 임의 문자열은 유지한다. 전체 커밋 출처·실행 파일 SHA-256·배포 검증 신원은 줄이지 않는다.
- `cargo test --locked -p aaa-common --lib software_tests`는 긴/짧은 생성 결과, 같은 버전·커밋 접두의 양방향 비교, 다른 커밋/버전/임의 salt, 같은 12자리 뒤가 다른 전체 커밋을 검사한다. host의 `machine_view_tests`는 실제 hello·DB 재시작 뒤에도 옛 긴 판을 짧게 제공하고 원문은 보존하는지 확인한다.
- `AAA_BUILD_SALT=0123456789abcdef0123456789abcdef01234567 cargo test --locked -p aaa --test software`는 실제 빌드한 CLI와 이름을 바꾼 setup 실행 파일의 `--version`이 `aaa 0.1.6+0123456789ab`인지 검사한다. 같은 salt로 `cargo test --locked -p aaa --bin aaa setup::tests::setup_records_the_current_short_build_version`는 격리 설치 흐름의 안내·영수증을 확인한다. OS 서비스는 가짜이며 실제 Windows 설치 검증을 뜻하지 않는다.
- E2E35의 기존 배포/전체 SHA/권한/초점/화면 조합 단언은 그대로 두고 짧은 판에 「host와 다름」이 붙지 않는 머신을 추가한다. Chromium·WebKit × 390/1280px에서 확인한다. Mac 빌드 스크립트의 기존 Python 계약도 12자리 salt·판을 검사한다. 완료는 프로세스 종료·DB 재열기·DOM 사건으로 판단한다.
- 참고 후보는 [Git의 커밋 접두 표기](https://git-scm.com/docs/git-rev-parse)([GPL-2.0](https://github.com/git/git/blob/master/COPYING))와 [Rust semver의 build metadata 비교](https://docs.rs/semver/latest/semver/struct.BuildMetadata.html)([MIT/Apache-2.0](https://github.com/dtolnay/semver))다. 접두 개념을 따르되, 같은 패키지 버전의 서로 다른 빌드도 구별해야 하므로 metadata를 무시하는 버전 요구식은 쓰지 않는다. 외부 코드·의존성 추가는 없다.

## Windows Python 셸 계약 시험의 경계(t-0984)

- `python -m unittest -v scripts.test_deploy_build scripts.test_check_public scripts.test_rust_tests scripts.test_release scripts.test_ci_local scripts.test_linux_cross`를 세 OS에서 실행한다.
  Windows는 네이티브 Python과 Git for Windows를 쓴다. `bash`를 PATH에서 그대로 찾으면 WSL 입구가 선택될 수 있으므로,
  `shell_test_support.bash_executable`이 PATH의 Git 설치 옆 Bash를 명시한다. Git Bash가 없으면 원인을 담은 오류로 실패한다.
- `.gitattributes`는 셸 스크립트와 확장자 없는 cross wrapper를 LF로 체크아웃한다.
  Windows의 `core.autocrlf=true`에서도 Bash가 CRLF를 명령 일부로 읽지 않아야 한다.
  Zig fixture도 LF로 쓰고 wrapper를 Bash의 인자로 전달한다. 공백 경로·달러 문자·각 인자·종료 코드 23의 단언을 유지한다.
- Windows에서 추가로 건너뛰는 것은 공개 검사의 개행·탭 파일명 시험 3건,
  가짜 Cargo와 시험 바이너리의 Unix shebang 실행 시험 1건,
  Unix 전용 CI 단계와 shebang 도구 시험 10건, Unix 컴파일러·linker PATH 시험 1건이다.
  각 시험 또는 클래스에 사유를 적는다. 기존 deploy-build·symlink·비 UTF-8 파일명 skip도 유지한다.
  로그 스트리밍 시험은 `CiStream`으로 분리하여 Windows에서도 UTF-8 청크 경계와 완료 후 drain을 실제 검사한다.
  공개 검사의 경로별 크기 상한, Rust runner의 Python 자식 병렬 실행, release 계약, Zig wrapper 검사는 Windows에서도 실행한다.
- 기준 main a17bce49의 Windows 재현은 40건 중 실패 8·오류 9·skip 10이다.
  옛 15건 목록 대신 이 결과를 기준으로 삼는다. 시한·기대값·재시도 정책과 제품 코드는 바꾸지 않는다.
  수정판은 Windows 15 통과·25 skip, macOS 39 통과·1 skip, Ubuntu 24.04 38 통과·2 skip이다.
  Unix에서는 기존 플랫폼 skip만 남는다. macOS 전달본의 CRLF 변환과 새 Ubuntu VM의 rustc·Node 누락을
  먼저 해결한 뒤 같은 시험을 실행했다. 전달용 `git archive`에는 `-c core.autocrlf=false`를 지정한다.
- 참고 후보는 [CPython의 조건부 skip](https://github.com/python/cpython/blob/main/Lib/test/test_subprocess.py)
  ([PSF-2.0](https://github.com/python/cpython/blob/main/LICENSE))과
  [Meson의 Windows 실행 파일·shebang 처리](https://github.com/mesonbuild/meson/blob/master/mesonbuild/programs.py)
  ([Apache-2.0](https://github.com/mesonbuild/meson/blob/master/COPYING))다.
  플랫폼 전제는 명시적으로 건너뛰고, 가능한 스크립트 호출에는 interpreter를 명시하는 경계만 참고했다. 코드 복사·의존성 추가는 없다.

## E2E16 긴 줄 바꿈 뒤 줄 참조 재열기(t-0978)

- [원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/40062175921/job/120777851872)은 SHA 1583a1ed에서 「접은 big.ts 2500번째 줄 가운데」 20초 만료다. 원문과 결과 artifact를 확보했으나 소스 행 좌표·파일 보기 화면·trace가 없어 당시 거짓 조건을 소급 확정하지 않는다. 수정 전 전체 E2E16과 가상화 단계 단독은 각각 한 번 통과했으며 이 통과를 결함 부재의 증거로 삼지 않는다.
- 설정의 실제 `history.go`를 viewer mount 뒤에 전달한 대조에서 원래 20초 실패가 재현됐다. DOM은 닫혔지만 설정 기록은 남은 상태에서 파일 열기가 새 기록을 쌓고, 늦은 native popstate가 파일 보기를 제거했다. 실패 당시 viewer=false·row 없음·viewport 없음·selected=false이고 설정이 다시 표시됐다. 가상화 행 측정이나 중앙 좌표 오차 전에 파일 보기 자체가 사라지는 시험 준비 순서 결함이다. 이 대조가 원본 CI 사건 순서와 같았다고 단정하지 않는다.
- 가상화 단계도 기존 `waitForSettingsExit`로 설정 DOM·history state·query가 모두 설정 밖인 사건을 기다린다. `settings-exit-gate.mjs`는 DOM 제거 뒤 실제 go를 보류한 채 waiter가 끝나지 않았는지 확인하고 native go/popstate를 풀어 소스를 연다. DOM 제거만 기다리던 변형은 `pending=-1·shown=false·entry/query=true·exited=true`로 즉시 실패해야 한다. 기존 단위 owner `web/test/settingsClosed.test.mjs`의 양방향 사건 순서·실패 상한·정리 검사도 유지한다.
- 설정 switch 반영 → 기록 복귀 → 소스 열기 → 선택 행 중앙 → worker 강조 반영 → 중앙 재확인 → 긴 행의 실제 높이/가로 넘침을 검사한다. 기존 20초·120초 상한, 행 수·선택·가운데 허용 폭·문법 색·기기별 설정·라이선스·휴대폰/iPad 조합을 유지한다. 재시도·기대값 완화·고정 sleep·제품 가상화 변경은 없다.
- `virtual-source-diagnostics.mjs`는 shadow 행 존재·선택·행/viewport 사각형·중앙 차이·scrollTop/Height·wrap/가로 넘침·긴 행 높이와 DOM/resize/scroll/popstate·파일 요청/클릭 순서를 관측한다. 바깥 단계 만료도 `diagnose`로 context 정리·공통 termDiag 전에 JSON/PNG를 보존한다. `AAA_FILE_VIEWER_TRACE=1`이면 해당 가상화 context의 screenshot/snapshot trace도 실패 산출물에 넣는다. 성공 촬영의 `desktop-file-virtual-wrap.json`에도 같은 측정을 남긴다. 구조화 JSON에는 소스 본문·전체 URL·임의 history state·cookie를 수집하지 않는다. trace에는 격리 실험실의 fixture 화면이 포함된다.
- 참고 후보는 [herdr-web-ui VirtualSource](https://github.com/devswha/ovis/blob/main/src/components/VirtualSource.tsx)([MIT](https://github.com/devswha/ovis/blob/main/LICENSE))와 [TanStack Virtual](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts)([MIT](https://github.com/TanStack/virtual/blob/main/LICENSE))이다. 줄 추정/재측정 뒤 이동을 대조하고, 이번 재현에서는 그 단계 이전의 기록 경계를 수리한다. 코드 복사나 외부 의존성 추가는 없다.
- 검증 범위는 `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 16 --workers 1`, 웹 유닛·typecheck·빌드와 공개 검사다. 무거운 전체 CI는 착지에서 수행하며 E2E 번호·FRONT_API 계약은 바꾸지 않는다.

## E2E102·E2E37 병렬 부하의 입력·위치 경합(t-0969)

- 원본 조건인 전체 E2E worker 10개에서 E2E102 WebKit 1280의 초안 유실을 재현했다. 입력 capture에는 새 글이 있었지만 React의 `updateTextarea` commit이 빈 값을 썼고, bubble `onChange`가 초안을 받지 못했다. 같은 글 30회 전송의 고정 4회 대조에서는 3회가 원래와 같은 보내기 단추 click 30초 만료였다. 실패 당시 입력칸·초안 저장은 비었고 중단 단추가 있어 locator가 찾는 보내기 단추가 사라졌다. 실패를 통과 표본으로 바꾸지 않는다.
- E2E102는 root의 native input capture 뒤 `mouseup` 선택 사건을 끼워 앞선 selection commit → input bubble 순서를 결정적으로 검사한다. 구판은 보내지 않은 초안이 비어 실패하고 수정판은 보존해야 한다. 자연 실패 당시 모든 commit의 기원을 소급 확정하는 것은 아니며 같은 덮어쓰기 경로를 제어한 회귀다. 제품은 활성 입력칸의 window input capture에서 새 글과 커서를 먼저 저장한다. React root의 처리보다 먼저 받아 뒤의 commit이 새 DOM 값을 덮지 않게 한다. 숨김·해제 때 listener를 정리하고 기존 change 경로도 유지하며 같은 글의 이중 저장은 피한다. 8조합의 PTY 수신 순서·ACK 전 다음 초안·늦은 기록·포커스·중복 전송 단언은 그대로다.
- E2E37은 제어 응답을 보류한 WebKit 1280에서 원래의 row40 `77.5 → 114`를 재현했다. 전후 scrollTop=3082·scrollHeight=248369이고, 대화 영역 y=97→133.5와 안내 띠 높이 36.5px가 같다. 스크롤 이동과 연결 안내의 화면 배치를 혼동한 시험 순서 결함이다. 실제 WS 응답을 복귀 DOM 검사 뒤에 전달해 띠가 있는 동안의 영역 안 행 위치도 1px로 검사하고, 제어권·띠 제거를 확인한 뒤 기존 절대 좌표 1px 단언을 유지한다. 제품 스크롤 코드는 바꾸지 않는다.
- 실패 시 context를 닫기 전에 DOM·화면·PTY 기록·입력 사건을 보존한다. `AAA_CHAT_TRACE=1`은 해당 대화 context의 screenshot·snapshot trace도 실패 산출물에 넣는다. trace가 순서에 영향을 줄 수 있어 원본 조건 대조는 trace 없이도 실행한다. 시한 확대·재시도·기대값 완화·시험 삭제·고정 sleep은 없다. E2E 번호와 FRONT_API를 유지한다.
- 먼저 [Playwright actionability](https://github.com/microsoft/playwright/blob/main/docs/src/actionability.md)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [TanStack Virtual](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts)([MIT](https://github.com/TanStack/virtual/blob/main/LICENSE)), [React ChangeEventPlugin](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/plugins/ChangeEventPlugin.js)·[UpdateBatching](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/ReactDOMUpdateBatching.js)([MIT](https://github.com/facebook/react/blob/main/LICENSE))를 대조했다. 공개 capture API와 기존 host의 제어 사건을 사용하며 외부 코드·의존성은 추가하지 않는다.
- 최종 검증은 전체 `nice -n 15 bash scripts/ci-local.sh`와 worker 10개 전체 E2E, Linux strict cross clippy·공개 검사다. 기존 E2E23·35·66의 IME·첨부·전송 높이·Enter 검사도 유지한다. headless WebKit의 주입 사건을 실제 iPhone 키보드 실기 통과로 해석하지 않는다.

## Ubuntu 진단 로그 capture의 첫 callsite 등록(t-0973)

- [원문 CI의 Ubuntu test](https://github.com/cLazyZombie/aaa/actions/runs/40041938212/job/120714799001)는
  5c66fdf3에서 `RUST_TEST_THREADS=8`로 `ci-local.sh --only test`를 실행했다. `rust_tests.py`가
  `cargo test --workspace --locked --no-fail-fast --no-run`으로 준비한 aaa-common 실행 파일 전체
  48개 시험을 필터 없이 실행했고, info 경로는 통과했지만 debug 경로는 1건·64건 모두 0건이었다.
- 당시 Cargo.lock과 같은 tracing 0.1.44·tracing-core 0.1.36을 사용한다. workspace feature에는
  `max_level_*`·`release_max_level_*`가 없다. Capture의 기본 `max_level_hint=None`은 TRACE이며,
  subscriber 등록은 OnceLock 한 번뿐이다. 원인은 subscriber 해제 경쟁이 아니라 tracing-core의
  단일 Dispatch 최적화다. 등록된 Dispatch가 하나면 `Rebuilder::JustOne`은 callsite를 처음
  실행한 thread의 기본 subscriber로 interest를 계산한다. capture 밖 thread는 NoSubscriber라서
  `observe`의 공통 debug callsite가 `never`로 굳고, 이후 `with_default`는 캐시를 재계산하지 않는다.
- `cargo test -p aaa-common --locked capture_after_an_uncaptured_thread_first_uses_the_callsite -- --nocapture`는
  같은 시험 실행 파일의 정확한 시험 하나를 새 프로세스로 실행한다. 빈 capture로 subscriber를
  준비한 뒤 capture 밖 thread가 `observe`를 먼저 실행하게 하고 `join`으로 등록 완료를 확정한다.
  수정 전에는 기존 비밀 payload 시험의 기대 1건이 실제 0건으로 실패했다. 수정 후에는 같은
  payload 시험과 기존 8 thread × 64건의 개수·OS 코드 분리 검사를 모두 통과해야 한다.
  동적 최대 level TRACE와 capture 밖 사건의 수집 제외도 검사한다. 고정 sleep·반복·시한 변경은 없다.
- 시험 전용 subscriber를 전역에 한 번 설치하고 수집 대상만 thread-local로 바꾼다. capture 밖 사건은
  필드를 포맷하지 않고 버린다. 기존 중첩 capture의 복원 guard와 비노출 단언은 유지한다.
  제품 subscriber·로그 수준·진단 본문·API는 바꾸지 않는다. macOS에서도 같은 순서를 재현하므로
  Linux VM·컨테이너는 만들지 않는다. 전체 로컬 CI와 Linux strict cross clippy로 함께 검사한다.
- 참고 후보는 [tracing-core의 callsite 구현](https://github.com/tokio-rs/tracing/blob/tracing-core-0.1.36/tracing-core/src/callsite.rs)
  ([MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE))와
  [tracing-test의 한 번만 설치하는 전역 subscriber](https://github.com/dbrgn/tracing-test/blob/main/tracing-test-macro/src/lib.rs)
  ([MIT 또는 Apache-2.0](https://github.com/dbrgn/tracing-test/blob/main/LICENSE-MIT))다.
  전역 초기화 방식만 참고하며 외부 코드나 의존성을 추가하지 않는다.

## Windows 한 줄 설치 입구와 install 시험 분리(t-0974)

- Windows job [120714799191](https://github.com/cLazyZombie/aaa/actions/runs/40041938212/job/120714799191)(5c66fdf3), [120727809482](https://github.com/cLazyZombie/aaa/actions/runs/40046017912/job/120727809482)(30ccda99), [120743045058](https://github.com/cLazyZombie/aaa/actions/runs/40050878199/job/120743045058)(daaa43f1)의 전체 원문은 모두 `install` target의 `0xc0000409` 비정상 종료다. 원문에는 해당 target의 `test result`·stage·PTY 진단이 없으므로 당시 stage를 소급 관측했다고 하지 않는다. quota 시험은 모두 최종 ok다.
- 5c66fdf3은 공개 bootstrap의 명령을 `install`에서 `setup`으로 바꿨지만 `recovery::one_line_entry_keeps_the_external_console_through_review_and_signed_file_installation`은 옛 첫 질문 「설치 방식」을 기다렸다. 같은 원본의 Windows `--nocapture` 실행에서 실제 「host 주소 (이름:포트)」와 질문 0/16, `shell=powershell.exe, stage=payload_written, http_phase=5, executor=true, config=false, active=false`를 확인했다. 60초 뒤 shell만 종료되어 남은 setup 자식이 실행 파일을 잡고 있었고, Lab 삭제의 Windows 오류 5가 두 번째 panic을 만들어 target을 abort했다. 4d78dc40의 HTTP FIN 수리는 세 SHA에 이미 포함된다.
- 옛 owner는 `recovery::one_line_entry_enters_setup_and_cancels_without_installation_changes`로 대체한다. 실제 PS5.1/7의 내장 `irm | iex`, 렌더한 공개 입구, PE·SHA 확인·별도 executor·기존 실행 파일 보존을 유지한다. 실제 setup 첫 질문에서 콘솔 EOF(Ctrl+Z Enter)로 취소하고 종료 130·호출 shell 복귀·설정/설치 기록 미생성을 검사한다. 외부 HTTPS 전송만 fixture로 격리하며 서비스·계정·LSA·PATH는 바꾸지 않는다. 비관리자 환경은 SSH 조건으로 UAC 창 없이 실패한다.
- 옛 16개 질문·review/yes·서명 파일 적용·단일 job·같은 ID의 status·기존 실행 파일 보존은 새 `recovery::external_console_reviews_and_installs_signed_files_without_service_changes`가 실제 `aaa install`과 기존 PTY driver로 검사한다. 설치 대상은 Lab 안에 복사한 별도 executor다. 설치는 실행 파일 옆에 소유 기록도 쓰므로 Cargo 실행 파일을 대상으로 삼으면 다음 빌드의 파일 교체와 충돌하고 시험 밖에 기록이 남는다. 기존 `authorization::issued_approval_runs_the_real_file_worker_and_status_keeps_the_public_id`의 서명·nonce·worker·status 검사도 유지한다. 제품 입구를 되돌리거나 질문/승인 단언을 없애지 않는다.
- Lab 정리가 unwind 중 실패하면 OS 오류를 남기고 원래 panic을 보존한다. 평상시 정리 실패는 계속 실패다. `cleanup_failure_preserves_the_original_panic_instead_of_aborting_the_test_binary`는 실제 삭제 공유 없는 Windows 파일 handle을 잡아 오류를 결정적으로 만들고, 원래 panic이 catch 경계로 돌아오는지 검사한다. 고정 sleep·재시도·시한 확대는 없다. HTTP 수명·모듈 검색 격리·PTY의 60초 상한도 유지한다.
- 먼저 [PowerShell WebRequest](https://github.com/PowerShell/PowerShell/blob/master/src/Microsoft.PowerShell.Commands.Utility/commands/utility/WebCmdlet/Common/WebRequestPSCmdlet.Common.cs)([MIT](https://github.com/PowerShell/PowerShell/blob/master/LICENSE.txt)), [portable-pty ConPTY](https://github.com/wezterm/wezterm/blob/main/pty/src/win/conpty.rs)·[PseudoCon](https://github.com/wezterm/wezterm/blob/main/pty/src/win/pseudocon.rs)([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)), [cargo-dist PowerShell 설치기](https://github.com/axodotdev/cargo-dist/blob/main/cargo-dist/templates/installer/installer.ps1.j2)([MIT 선택](https://github.com/axodotdev/cargo-dist/blob/main/LICENSE-MIT))를 대조했다. 응답 수명·콘솔 소유·호출자 복귀 경계를 참고했으며 새 외부 코드·의존성은 없다.
- Windows 실기에서 웹 타입 검사·빌드·유닛 544개(기존 제외 2개), release 별도 빌드, workspace/all-targets 빌드, strict clippy, 전체 Rust 1,096개(기존 제외 4개, install 30개 포함), fmt·공개 검사가 통과했다. 첫 전체 실행은 새 서명 owner가 Cargo binary 옆에 남긴 소유 기록과 다음 빌드의 충돌로 실패했다. 소유 키가 자기 Lab과 일치함을 확인해 그 기록 두 파일을 정리하고 설치 대상을 Lab 사본으로 고친 뒤, 웹 소스는 그대로 두고 `ci-local.sh --only fmt,build,clippy,test,public`로 관련 전체 단계를 검증했다. 단일 전체 실행의 실패 로그를 성공으로 바꾸지 않는다. Windows 규칙상 웹 E2E는 미실행이며 macOS/Linux 실행은 이 검증에 포함하지 않는다. 서비스·예약 작업·Run 키·사용자 PATH·방화벽은 전후 기준 목록이 모두 같았다.

## 자동화 Codex profile의 계정별 사용량(t-0970)

- `cargo test -p aaa-host quota_gate`: 세션 5%·주간 10%는 남은 비율이며 경계와 같을 때 허용하는지 확인한다. 주간 창만 있는 계정의 세션 문턱은 확인 불가로 남기고, 설정된 창만 검사한다. 실패·오래된 관측·리셋 경과·충돌·모델별 별도 몫·비정상 숫자를 허용하지 않는다. 로컬 공급자·계정을 섞어 누락을 채우지 않는다.
- 실제 Host·DB와 기존 agent frame fixture로 원격 Codex의 기본 계정, 재사용 thread의 고정 계정, 이전 로그인 세대 배제, 입력 보존·회복·중복 영수증을 검증한다. 진행 중인 계정 수집 잠금을 잡은 상태에서도 마지막 완료 캐시의 판정을 마쳐야 한다. 정상 회복 뒤 파일 전달은 한 번이고 새 thread는 만들지 않는다. 실제 계정·구독 요청·고정 sleep·재시도는 없다.
- `cargo test -p aaa-host automation::`로 기존 자동화 회귀를, `cargo test -p aaa-host ticker_refreshes_usage_without_a_browser_and_releases_pending_input`로 브라우저 없는 갱신·구독 응답 지연·입력 보존 회귀를 검사한다. UI·wire 형식은 바꾸지 않아 새 브라우저 E2E를 추가하지 않는다. 운영 자동화 파일·승인·문턱은 시험에서 변경하지 않는다.
- [CodexBar의 Codex 자료원](https://github.com/steipete/CodexBar/blob/main/docs/codex.md)([MIT](https://github.com/steipete/CodexBar/blob/main/LICENSE))의 계정별 구독 창·CLI RPC와 [ccusage의 Codex 로그 집계](https://github.com/ccusage/ccusage/blob/main/docs/guide/codex/index.md)([MIT](https://github.com/ccusage/ccusage/blob/main/apps/ccusage/LICENSE))를 비교했다. 구독 문턱은 기존 계정 RPC 캐시를 재사용하고, 토큰·비용 추정으로 대신하지 않는다. 외부 코드·의존성은 추가하지 않는다.

## E2E108 보고서 필터의 첫 화면 복원(t-0997)

- 57회차의 새로고침 뒤 `data-repo` 빈 값 실패를 조사했다. 원래 단언만 둔 현재 후보의 E2E108·105·91·109 병렬 3회는 통과했다. 이후 MutationObserver로 첫 카드 DOM을 관측하니 현재 후보·t-0999/t-1001 통합 조합 각각 3/3회, 변경 전 기준 main `04dd0417`에서도 저장값 `aaa`·주소 `#reports`인 채 필터 빈 값/카드 3개 → 필터 `aaa`/카드 1개의 순서가 나왔다. 강화한 첫 DOM 단언은 모두 RED다. 원본 착지의 정확한 사건 시각을 소급한 것은 아니며 동일한 잘못된 중간 상태를 직접 확인했다.
- 원인은 공통 `useRepoFilter`가 프로젝트 key 변경 뒤 `useEffect`에서 기억을 읽는 것이다. 보고서 목록 응답이 처음 프로젝트를 정하므로 카드가 먼저 전체로 commit된다. 기준 main에도 같은 경합이 있어 Ctrl+Tab의 hash 복원 변경과 독립이다. hook 초기화와 key 변경 render에서 기억을 읽고, 같은 key의 재렌더·선택에는 메모리 상태를 유지한다. 저장 거부·전체 선택·정하지 않음·보고서/할 일/다른 프로젝트 key 분리는 유지한다.
- E2E108의 원래 카드 대기와 즉시 `data-repo === aaa` 단언은 유지한다. 새로고침 전에 관측기를 설치해 첫 카드부터 필터 `aaa`·카드 1개인지 추가 검사하고 실패 시 시각·필터·개수·저장값·hash를 남긴다. 필터가 맞을 때까지 기다리는 우회·시한 확대·실패 재시도는 없다. Chromium·WebKit의 1280/820/390px와 Reader, 세 탭·읽음 범위·선택 초기화 검사를 그대로 돈다.
- 수정 후보에서 E2E108·105·91·109와 공통 칩을 쓰는 E2E76·77을 worker 10개로 3회씩 실행해 18개 작업 전부 통과했다. 웹 유닛 549개·타입/번들/host 빌드·공개 검사도 통과했다. 반복은 미리 정한 부하 표본이며 실패를 성공으로 대체하지 않는다.
- [React의 key 변경 시 render 안 상태 조정](https://react.dev/learn/you-might-not-need-an-effect#adjusting-some-state-when-a-prop-changes)과 [useLayoutEffect](https://react.dev/reference/react/useLayoutEffect)를 대조했다(React 구현 MIT). DOM 치수 측정이 아닌 기기 기억 초기화이므로 전자를 따랐다. 기존 App의 render 시 보관 목록 조정과 같은 공개 React 동작이며 외부 코드·의존성을 추가하지 않는다.

## E2E105 앱 화면 방문과 coordinator 구별(t-0997)

- 57회차 통합 후속: t-0998 `0e2db847`과 합친 상태에서 E2E91의 기존 20초 단언 실패를 재현했다. 종료된 pane의 목록 줄은 0개지만 `[data-origin="coordinator"]`에는 보이는 `SECTION.term` 1개가 남았다. `.terminal-preview`는 0개였으므로 표시 사본을 원인으로 볼 수 없고, 종료 기록 본문의 역할 표시와 목록 줄 판정이 같은 속성을 쓰는 충돌이다. 본문·최근 화면의 표시 속성을 `data-screen-role`로 분리하고 Sidebar의 `data-origin` 및 E2E91의 원래 종료 단언은 그대로 둔다. 실패 때 목록 줄 수·일치 DOM·사본 수를 남겨 재진단할 수 있게 했다.
- 통합 수정 뒤 E2E91·105·109·93을 worker 4개로 한 번 실행해 4개 파일·19개 작업을 모두 통과했다(E2E93 16단계 전체, E2E109 일반 이동·Ctrl+Tab 복귀 24회). 웹 유닛 549개·타입/번들/host 빌드·공개 검사도 통과했다. 기존 종료 조건·20초 상한은 바꾸지 않았다.

- 기존 전환기는 palette의 실행 중 pane만 후보로 만들고 `target.pane.id`만 열었다. 보고서·할 일 방문은 기록되지 않고 설정은 전환 자체를 막았다. 수정 전 실제 host·브라우저에서 보고서→할 일→Ctrl+Tab을 눌러 coordinator만 나오는 화면과 「방문한 보고서 목록 누락」 실패를 재현했다.
- 같은 E2E105에 Chromium·WebKit × 1280·390px의 보고서·할 일·자동화·설정·답할 것·작업 공간 방문→목록 포함→Ctrl 해제 이동을 추가한다. 개별 보고서 본문·공유한 할 일 상세 주소 재열기, 넓은 화면의 할 일 선택 보존, 설정에서 전환/단축키 기록 우선, Ctrl 유지 중 주소·MRU 불변, coordinator 머리/목록/전환기 표시와 밝은·어두운 테마의 가로 넘침을 검사한다. 기존 셸 키 누수·Esc·blur·종료·일반 탭/끄기 검사는 유지한다.
- `node --test web/test/recentScreens.test.mjs web/test/recentHistory.test.mjs`는 앱 목록·상세 주소 보존, 임의 URL 제외, pane 생명주기 필터와 앱 화면의 분리를 함께 확인한다. 고정 sleep·시한 확대·실패 재시도는 없다. 실제 설치 PWA의 OS 키 전달은 기존처럼 운영에서 사용자가 확인한다.
- 참고 구성: [VS Code MRU](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/editor/editorActions.ts)(MIT)의 확정 전 순서 유지, [herdr-web-ui Sidebar](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/Sidebar.css)(MIT)의 기존 토큰과 선택색을 보존하는 얇은 테두리, [herdr-projects popup](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/popup.rs)(MIT)의 역할/화면 분류. 외부 코드를 복사하거나 의존성을 추가하지 않는다.
- 자동화 목록을 본문으로 바꾼 초기 구현은 E2E26의 본문 유지와 E2E40의 휴대폰 메뉴 동작을 깨뜨려 제거했다. 기존 패널 동작에 방문 기록만 붙였고 두 시험을 통과했다. 같은 페이지의 늦은 hashchange는 다음 패널 방문을 지우지 않는다. 시험의 휴대폰 task 상세는 주소 없는 시트가 이동 시 닫히는 기존 계약과 공유 주소의 복원을 구분한다. 촬영은 시트 닫힘 그림의 animationend·DOM 제거를 기다린다.
- coordinator 복귀의 읽기 전용·빈 본문 문제는 사용자 정정에 따라 별도 d-0275 범위다. 여기서는 Term의 역할 속성과 표시 글자만 바꾸며 연결 로직은 다루지 않는다.

## E2E105 설치 앱의 최근 화면 전환(t-0934)

- `node --test web/test/recentScreens.test.mjs web/test/recentHistory.test.mjs`: 실제 palette 대상 모델로 thread/pane 별칭·실행/종료·보류/해결/보관·끊김, 방문 키 중복 제거·128개 한도·깨진 저장·저장 거부 시 메모리 유지·다른 창의 변경/삭제, 고정 MRU의 순환·역방향·0/1개·후보 제거를 확인한다. 완료 보고의 idle/done은 pane 종료로 보지 않는다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 105 --workers 1`: 실제 host·agent·키 입력을 보이는 PTY 셋을 만든다. Chromium(Win32)·WebKit(MacIntel)의 standalone 조건을 주입하고 Playwright 키 입력으로 순환·반대·Ctrl 확정·Esc 취소·blur 뒤 늦은 Ctrl 뗌·MRU 유지/확정·새로고침·팔레트 공유·선택 후보 종료를 검사한다. 설정 UI에서 끈 PWA와 일반 탭이 동일한 xterm bytes를 보내는지 대조한다.
- 전환을 시작할 때 실제 `.xterm-screen`을 클릭한다. 앱→pane WebSocket 입력 기록을 동작 전후 대조해 Tab·Shift+Tab·Esc가 전환 중 PTY로 새지 않는지 검사한다. DOM·주소·키 사건·pane 종료를 기다리며 고정 sleep·재시도·시한 연장은 없다. 스크린샷은 `recent-chromium`·`recent-webkit`이다.
- 이 자동 시험의 Chromium 입력은 CDP, WebKit 입력은 해당 엔진 protocol을 거친다. OS/브라우저의 예약 키 경로와 실제 설치 PWA는 검증하지 않는다. 최신 사용자 지시대로 별도 시험 host는 띄우지 않고 착지·배포 뒤 Mac 운영 PWA에서 사용자가 직접 확인한다. Windows Edge/Chrome OS 입력도 미확인으로 남긴다. 자동 시험 통과를 실기 통과로 보고하지 않는다.
- 기존 E2E03·04의 단축키·팔레트도 전체 로컬 `ci-local`에서 유지한다. wire/API와 기존 E2E 번호는 바꾸지 않는다. 구현은 plan §8.132의 설치 앱 전환 절을 따른다.

## 보고서 모두 읽음(t-0952)

- `node web/e2e/web.e2e.mjs --spec 101 --workers 1`: 실제 host DB의 내 보고서 105개와 별도 보관 판을 사용한다. Chromium·WebKit 1280·390px에서 버튼·확인 창을 캡처하고 확인 개수·취소의 무요청·오류 보존·빈 탭 비활성·한 요청·첫 50개 밖 처리·보관함 범위·활동/탭 배지·Reader 숨김·가로 넘침을 검사한다. DOM·응답이 완료 사건이며 고정 대기와 실패 재시도는 없다.
- host `mark_read_batch_is_atomic_idempotent_and_preserves_new_reports`는 Reader의 기존 열람 권한, 잘못된 항목의 부분 쓰기 방지, 중복/재요청 멱등, 새 판·새 보고서 보존, 배지 사건과 실제 열람 시각 보존을 검사한다.
- 참고는 [herdr-web-ui ConfirmDialog](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/ConfirmDialog.tsx)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)), [Miniflux app.js](https://github.com/miniflux/v2/blob/main/internal/ui/static/js/app.js)([Apache-2.0](https://github.com/miniflux/v2/blob/main/LICENSE))다. 기존 확인 창을 재사용하며 새 외부 코드·의존성은 추가하지 않는다.
## inbox 턴의 대화 입력 잠금 제거(t-0955)

- `node web/e2e/web.e2e.mjs --spec 102 --workers 1`은 실제 host·agent·PTY의 가짜 Claude에 `pane run`으로 inbox 안내를 주입한다. 이 호출은 nudge와 같은 `send_prompt` 길이다. 모델 대신 기록 반영만 보류하고 사람 글 두 개의 PTY 수신 순서·대기 말·미전송 초안·포커스를 검사한다. coordinator/일반 pane × Chromium/WebKit × 1280/390px의 8조합이다. 같은 글 두 번의 개별 수신도 확인한다. 원본은 첫 전송 뒤 readOnly=true로 실패했다.
- E2E23의 긴 글 hash·내부 알림 제외 단언은 잠금 대신 대기 말의 수신 여부를 검사한다. 전송 전 취소/Enter 차단·첨부·Codex·steering·compaction 검사는 유지한다. E2E35는 ACK 뒤 빈 초안이 한 줄로 줄어드는 동작을 반영하며 한 frame 높이 증가·상태 글 노출·IME 검사를 유지한다. 기존 잠금 풀기 시험은 제거한 제품 단계에 맞춰 늦은 수신이 새 초안을 보존하는 시험으로 대체한다.
- 참고 후보: [herdr-web-ui Composer](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/Composer.tsx), [pending-input](https://github.com/devswha/herdr-web-ui/blob/main/server/pending-input.ts)([MIT](https://github.com/devswha/herdr-web-ui/blob/main/LICENSE)); [herdr-projects nudge](https://github.com/eliasstravik/herdr-projects/blob/1f05c002f22c8355733c567ef47c511612bcdb1f/src/steps.rs), [prompt](https://github.com/eliasstravik/herdr-projects/blob/1f05c002f22c8355733c567ef47c511612bcdb1f/src/threads.rs)([MIT](https://github.com/eliasstravik/herdr-projects/blob/1f05c002f22c8355733c567ef47c511612bcdb1f/LICENSE)). 편집 가능한 초안과 작업 중 입력을 따르며 별도 서버 대기열은 도입하지 않는다.
- 실제 유료 모델의 응답/도구 호출 지연과 iPhone 실기 자판은 이 headless 시험으로 확인했다고 보지 않는다. 고정 sleep·시한 연장 없이 PTY 출력·ACK·DOM으로 끝낸다.
## E2E53 직접 답과 늦은 이전 답 응답(t-1041)

- 실제 host의 `thread_answer` 응답을 만든 뒤 전달만 보류한다. 새 보고의 질문 ID가 DOM에 나타나면 직접 답을 입력하고, 이전 응답을 풀어 본문 소비·렌더 사건 뒤 최신 thread 사건을 한 번 더 보낸다. 이전 응답이 최신 상태를 덮으면 카드가 제거되고 다시 생기며 입력이 빈 값·보내기가 비활성으로 바뀐다. 원래 제품의 CPU 4배 감속·세 실험실 병렬 대조에서 3/3 재현했다. t-1040 원본에는 응답/DOM 순서 추적이 없어 당시 순서 전체를 소급 확정하지 않는다.
- 답 응답을 기다리며 받은 같은 프로젝트·thread의 사건 또는 사건과 합친 재연결 목록을 보존한다. 기존 `ThreadReload`의 사건 우선 규칙을 답 응답에도 적용하며 서버 시각 비교나 새 프로토콜은 쓰지 않는다. 새 사건이 없으면 답 응답을 그대로 반영하고 실패·완료 때 관측을 해제한다. `threadReplies.test.mjs`는 갱신 없음·최신 사건·프로젝트 분리·겹친 요청·해제를 검사한다.
- E2E53의 새 회귀는 Chromium/WebKit × 1280/390px에서 같은 DOM 유지·분리 없음·입력 보존·보내기 활성, 각 보고 hash에 맞는 답 요청 한 번과 실제 PTY/저장 답을 확인한다. 실패 자료에는 요청 hash·글과 카드 DOM을 남긴다. 원래 선택지 없는 카드 대기도 질문 ID와 비활성/선택지 수를 한 DOM 관측에서 확인한다. 기존 형식 경고·직접 답·inbox·원문·Reader·CLI 시나리오는 유지한다.
- 좁은 검증은 `AAA_E2E_THROTTLE=4 node web/e2e/web.e2e.mjs --spec 53 --grep '늦은 답 응답' --repeat 3 --workers 3`, 전체 부하는 `nice -n 15 bash scripts/ci-local.sh`와 E2E53 전체 반복을 함께 쓴다. Playwright 1.64.1 npm 패키지만 작업 폴더에 두고 기존 공용 Chromium/WebKit 캐시를 쓴다. 고정 sleep·재시도·시한 연장·기대값 완화는 없다.
- 먼저 [React의 key와 상태 수명](https://react.dev/learn/preserving-and-resetting-state)([MIT](https://github.com/react/react/blob/main/LICENSE))과 [Playwright의 실제 응답 전달](https://playwright.dev/docs/api/class-route#route-fetch)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))을 대조했다. 카드 재생성의 상태 초기화와 응답 gate를 참고하고 외부 코드·의존성을 추가하지 않았다.
- 첫 추가 전체 부하의 한 회는 새 회귀의 첫 답 응답 시한에서 진단 전에 종료됐다. 응답 gate의 시한을 입력 준비 뒤에 걸고 누름·응답을 `Promise.all`로 함께 관측해, 기다리는 누름보다 먼저 거부돼도 진단·정리를 마친다. 기존 20초 응답·30초 누름 상한은 유지하며 실패 자료에 실제 사건·요청·입력 상태를 보존한다.
- 수정본 첫 전체 gate의 E2E53은 통과했지만 E2E113의 사본/링크 fixture가 SQLite 잠금으로 실패했다. 보존 DB에는 사본 500개·링크 356개가 있어 개별 링크 INSERT 구간의 외부 writer 경합을 확인했다. 사본·링크 준비와 링크 정리는 host 종료 뒤 transaction으로 실행하고 UI 결과는 읽기 전용 연결로 본다. 내부 보고서 준비는 바깥 transaction과 함께 쓸 수 있는 savepoint를 쓴다. `bulk-fixture.mjs`·그 시험은 t-1038 후보의 동일 파일을 재사용하며, 기존 실제 500개 정리·키 교체·DB/파일/UI 단언은 유지한다. [SQLite의 단일 writer·savepoint](https://www.sqlite.org/lang_transaction.html)([public domain](https://www.sqlite.org/copyright.html))를 대조했으며 busy 재시도·시한 증가는 없다.
- 첫 host 재시작 대조는 빈 manifest를 구형 보고로 읽는 `backfill_report_ids`가 판 사본을 추가해 1,000개 정리로 실패했다. fixture의 빈 manifest는 기동 마이그레이션이 끝난 뒤 두고, 원래 500개 사본·링크·최종 파일/DB 단언을 유지한다.
- 최종 전체 실행은 웹 유닛 581개·fmt·build·clippy·Rust 1,342개(기존 ignored 6)·Python 56개(기존 skip 2)·public이 통과했다. E2E 114파일/140작업 중 E2E53·113을 포함한 139작업은 통과했고, E2E114의 500개 변환 한 작업이 기존 20초 응답 상한을 넘었다. 보존 파일의 원문 이동 12.0초·규칙 478개 작성 9.3초와 해당 load1 22.9~27.8을 기록했다. 같은 코드·시한의 분리 E2E114는 네 조합 전체가 88.9초에 통과했고 실제 변환 응답은 10.6~11.5초였다. 이 분리 통과는 착지 근거로 쓰지 않는다. 아래 제품 동기화 수리 뒤 E2E53·113·114를 포함한 전체 10 worker gate 통과가 필요하다.
- 같은 전체 gate와 함께 CPU 4배 감속·E2E53 전체 세 회(worker 3)를 실행해 3/3 통과했다(98.9초). 원래 형식/직접 답 시나리오와 새 네 화면 응답 역전 검사를 모두 유지했다. 추가 실행 동안 load1은 10.3~48.7이었고 전체 gate와 실행 파일 바이트가 같았다. 묶음 착지에서는 t-1038과 함께 전체 gate를 다시 확인한다.

## 이전 자동화 500개 변환의 동기화 비용과 복구(t-1041)

- `aaa_conversion` 로그의 info는 미리보기·실행·재개 단계별 횟수/합계/최대 시간과 경과 시간을, debug는 파일별 쓰기·파일 fsync·원자적 공개·디렉터리 fsync·원본 제거 비용을 기록한다. 실행에는 잠금·계획 준비(디렉터리 읽기/파싱/검증)·기존 상태/DB 조회·journal 저장을, 재개에는 경로/내용 검증·파일 작업·DB/상태 반영을 구분한다. 원문 내용은 기록하지 않는다. E2E114는 성공한 실험실도 삭제 전에 단계 합계를 전체 gate 로그에 남긴다.
- 실제 host의 같은 500개 시나리오에서 수정 전 HTTP 9,026ms, 파일 재개 8,844ms였다. 파일 fsync 1,001회 합계 3,930ms와 디렉터리 fsync 1,001회 합계 3,947ms가 재개의 약 89%였다. 계획 58ms·상태 포함 31ms·journal DB 6ms·재개 검증 10ms로 DB/스캔은 주 병목이 아니었다. macOS Rust의 `sync_all`은 `F_FULLFSYNC`를 사용한다.
- 파일마다 `sync_all`을 유지하면서 파일 핸들과 동기화 작업을 최대 8개로 제한해 함께 실행한다. 모든 파일 동기화가 성공한 묶음만 hard link(보존 사본)/rename(출력)으로 공개한다. 같은 폴더의 생성은 한 번만 하며 변경 폴더와 조상을 각 단계 끝에 중복 없이 자식부터 동기화한다. 보존 사본의 데이터·이름 → 출력 데이터·이름 → 원본 제거·폴더 동기화 순서이며, 결과 파일이 모두 내구화되기 전에는 원본을 제거하지 않는다. 원문 제거 직전과 출력 공개 직전에 다른 편집을 재확인한다.
- 첫 개선 계측은 HTTP 2,437ms, 재개 2,271ms였다. 파일 fsync는 1,001회 그대로이며 디렉터리 fsync는 보존 3회·출력 2회·제거 2회로 줄었다. 병렬 파일 fsync 합계는 worker별 경과의 합으로 전체 경과와 구별한다. 이 좁은 계측은 원인/개선 확인용이고 착지는 전체 `AAA_E2E_WORKERS=10 nice -n 15 bash scripts/ci-local.sh`에서 판단한다. 500개·응답 20초·최종 파일/DB/UI 단언을 유지한다.
- `cargo test --profile verify --locked -p aaa-host --lib automation::tests::`는 기존 계획 hash·Reader·경로/심볼릭 링크·두 판 원문 보존과 새 묶음 회귀를 함께 검사한다. 새 회귀는 17개로 여러 묶음을 넘겨 보존/출력 내구화 순서, 보존 폴더 동기화 전·9번째 출력 공개·출력 폴더 동기화 전·첫 원본 제거에서의 중단, 실제 DB journal 유지와 host 재개·완료 뒤 멱등성을 확인한다. 실제 hard link 충돌 때 기존 사본·원문을 보존하며 자기 임시 파일만 정리한다. 검증 뒤 생긴 다른 편집을 출력이나 제거가 덮지 않아야 한다. 오류에는 원인과 보존/출력/제거 확인 개수를 남긴다.
- 먼저 [atomicwrites의 파일/상위 폴더 동기화와 macOS flush](https://github.com/untitaker/python-atomicwrites/blob/master/atomicwrites/__init__.py)([MIT](https://github.com/untitaker/python-atomicwrites/blob/master/LICENSE))와 [tempfile의 persist 내구성 경계](https://docs.rs/tempfile/latest/src/tempfile/file/mod.rs.html)([MIT](https://github.com/Stebalien/tempfile/blob/master/LICENSE-MIT))를 조사했다. 데이터와 이름의 내구성을 별도로 보장하는 순서를 참고하며 외부 코드나 의존성은 추가하지 않았다.

## 앞 기록 재구성 뒤 마우스 좌표 인코딩(t-1041)

- 변환 수리 뒤 첫 전체 10 worker gate는 E2E53·113·114가 통과했으나 E2E109 WebKit 390px 복귀 첫 휠에서 실패했다. 실제 수신 자료는 휠 무전송이 아니라 `bin`의 옛 `ESC[M` 보고 한 건이었다. 앞 기록 적용 뒤 `addon-serialize` 0.13이 마우스 추적만 직렬화하고 SGR 좌표 인코딩을 빠뜨려, `term.reset()` 뒤 보고 형식이 바뀌었다.
- E2E109는 실제 앞 기록의 `historyApplied` 사건과 맨 아래 복귀를 먼저 기다린 뒤 같은 12회 이동·첫 SGR 휠/실제 PTY 정확히 한 번·동일 xterm·연결·snapshot·입력·Reader·만료 단언을 검사한다. 고정 sleep이나 시한 변경 없이 원래 제품의 첫 이동에서 결정적으로 실패한다. 제품은 재구성 전에 xterm의 현재 좌표 인코딩을 읽어 SGR/SGR-pixels 모드도 직렬화 본문에 붙인다. 옛 기본 인코딩에는 모드를 추가하지 않는다.
- [xterm serialize addon의 `_serializeModes`](https://github.com/xtermjs/xterm.js/blob/5.5.0/addons/addon-serialize/src/SerializeAddon.ts)와 [CoreMouseService](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/common/services/CoreMouseService.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)) 및 설치된 같은 판의 소스를 대조했다. xterm 내부 접근은 기존 viewport 보정처럼 고정 판에 한정하며 실제 브라우저/PTY 회귀가 판 변경 때도 보존을 검사한다.

## 질문 형식 경고와 답 전송(t-0956)

- r-1931·r-1994에서 `## Questions` 절을 수정 없이 fixture로 보존한다. `A를 달성한 사건`과 `t3code 고정 판`을 각각 A·T 후보로 센 것이 원인이다. 한글·숫자까지 단어 경계에 포함하여 두 원문 모두 질문 1개·선택지 2개·추천 A를 읽고 경고가 없어야 한다.
- proto 회귀는 두 원문과 A안·B안·A1을 검사한다. 기존 누락·글자 순서·상한·강조·fence 시험은 유지한다. 머리를 하나도 못 읽으면 원문을 둔 직접 답 카드 하나를 남긴다.
- E2E53은 실제 host·agent에서 경고 카드 직접 답, 선택지 없는 카드 직접 답, 두 원문의 Chromium·WebKit 카드 B 답, 경고 카드의 짧은 질문 ID CLI 답과 정상 M3 질문을 검증한다. 답 저장·PTY 출력이 완료 사건이다. 기존 답 차단 단언은 의도적으로 답 허용 단언으로 대체한다. Reader 권한·낡은 hash·중복 답·없는 선택지 검사는 그대로다.
- [herdr-projects v0.2.35 threads](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/threads.rs)·prompt_box와 [MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)를 확인했다. 이 판은 pane 입력·질문 상태를 다루며 aaa의 Markdown 질문 카드 파서에 해당하는 구현은 없다. [marked의 block 규칙](https://github.com/markedjs/marked/blob/master/src/rules.ts)과 [MIT](https://github.com/markedjs/marked/blob/master/LICENSE)도 대조했다. 목록 표시와 선택지 문법을 구별하는 원칙만 참고하고 코드·의존성을 추가하지 않는다.
- 로컬 전체 `nice -n 15 bash scripts/ci-local.sh`, 기존 E2E53, 공개 검사를 실행한다. FRONT_API·E2E 번호와 wire 필드는 유지한다. 경고는 thread inbox로 전달하며 사용자 답을 가로막지 않는다.
- 실행 기록: 첫 ci-local은 전역 Playwright 1.59.1 때문에 E2E 시작 전에 멈췄다. 작업 폴더에 1.64.1을 설치하고 같은 CI 실행 파일 사본으로 전체 102파일·128작업을 실행했다. E2E53 포함 127작업 통과, E2E72는 고부하 중 223MiB SSH 전송이 화면 대기 30초를 넘었다(계획 시작 뒤 파일 마지막 쓰기 31.7초, 당시 관측 load 89~330). 분리 실행은 예상한 SSH 거부까지 갔지만 첫 실패의 사본이 정리 검사에 걸렸다. SHA·생성 시각으로 자기 사본을 확인해 정리한 뒤 동일 코드·시한·단언으로 전체 E2E72가 13.3초에 통과했다. 시한·기대값·시험 코드는 바꾸지 않았다. 웹 유닛 533개, Rust 1,278개(기존 제외 6개), 의존 경계, native/Linux strict clippy와 공개 검사도 통과했다.
## iPad 할 일 행의 열 정렬(t-0959)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 107-task-row-layout --workers 1`은 실제 host의 일반·예약·하위·병합 대기 행을 만든다. 모델·머신·저장소·메모·첨부·연결 thread를 함께 표시한다.
- WebKit 1366×1024·1024×1366·390×844와 Chromium 1280×900·1920×1080에서 상태 원·ID·제목·메뉴의 x좌표, 표시된 칩·표지·제목의 교차 면적, 행 경계와 가로 overflow를 검사한다. 표지 글의 잘림, 첨부 3개, 제목으로 상세 열기·닫기도 확인한다. 글꼴 준비·host 보고 단계·DOM을 완료 사건으로 쓰며 고정 대기는 없다.
- 전체 검증에서 E2E100은 auto mode 표지가 보인 직후 아직 pane 구독이 연결되지 않아 `재연결 중…` 문구와 정상 placeholder를 비교했다. `ChatComposer`의 `connected`로 활성화되는 첨부 단추를 완료 사건으로 기다린 뒤 기존 문구를 그대로 검사한다. 제품·기대값·시한은 바꾸지 않는다.
- E2E43의 긴 제목 말줄임 검사를 유지한다. 하위 일은 행 전체 패딩 대신 실제 제목 글의 시작 위치가 안쪽이고 ID 열은 상위와 같아야 한다. 행 전체 들여쓰기가 열을 어긋나게 했던 기존 검사와 제품 배치를 함께 바꾼다.
- 기존 E2E43·47·52·67·76의 메뉴·thread 이동·예약 끌기·순서·첨부 조작을 유지한다. 수정 전 같은 화면 크기의 열 어긋남을 RED로 기록하고, 전체 로컬 `ci-local.sh`로 검증한다. API는 변경하지 않는다.
- [herdr-web-ui Sidebar.css](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/Sidebar.css)의 공통 선행·제목·후행 열과 축소 가능한 제목을 참고했다([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)). [herdr-projects overview.rs](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/overview.rs)의 ID·제목·상태·위치 나열도 대조했으나 텍스트 출력이라 반응형 배치에는 쓰지 않는다([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)). 외부 코드나 의존성을 추가하지 않는다.

## 휴대폰 기기 정보의 뒤로·앞으로와 목록 스와이프(t-0936)

- `node web/e2e/web.e2e.mjs --spec 98 --workers 1`: 실제 host·agent를 쓰고 Chromium·WebKit 390px에서 기기 이름을 누르면 `#machine=<id>` 한 칸만 더해지는지 검사한다. 가장자리 touchstart/move/cancel 뒤 native `history.back()`으로 원래 thread 목록에 돌아가며, 앞으로 같은 기기 정보가 다시 보인다. 두 번 왕복한 브라우저 기록을 기존 history observer로 비교한다(Chromium은 CDP의 실제 기록·위치, WebKit은 기록 길이).
- 화면 안쪽에서 시작한 손가락을 중간에 붙잡아 목록의 실제 이동 거리와 `elementFromPoint`로 목록이 정보 패널 위에 드러남을 확인한다. 취소 뒤 목록이 닫히고 기록은 그대로여야 한다. 머리의 기존 목록 단추, 새로고침 뒤 주소 복원, 닫기도 검사한다. 기존 1280·390px 양 엔진의 판·정보·테마·Esc·머신 전환·구판·끊김 검사를 유지한다.
- 수정 전에는 첫 기기 클릭 뒤 주소가 `#threads`에 머무는 RED를 확인했다. 기기 정보만 별도 고정 패널이라 목록보다 위에 겹치는 원인도 제거한다. 768px 이하에서는 기존 본문·drawer에 놓고 주소 기록만 사용한다. 데스크톱에서 클릭해 여는 420px 패널과 정보 행은 유지한다.
- 참고 후보는 [herdr-web-ui edgeSwipe](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/edgeSwipe.ts)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE))와 [remix-run/history](https://github.com/remix-run/history/blob/main/packages/history/index.ts)([MIT](https://github.com/remix-run/history/blob/main/LICENSE))다. 이미 옮겨 둔 목록 스와이프와 PUSH/POP 구분을 따르며 외부 코드·의존성은 추가하지 않는다.
- 완료 사건은 popstate·DOM·drawer transition이다. 고정 sleep·재시도·시한 연장 없이 로컬 전체 ci-local·Linux strict cross clippy·공개 검사를 수행한다. headless WebKit에서 주입한 touch와 기록 이동은 실제 iPhone OS의 미리보기 애니메이션을 확인한 것과 구별한다. t-0935와 겹칠 수 있는 파일은 `App.tsx`, `MachineInfoPanel.tsx`, `MachineInfoPanel.css`, 이 E2E98이며 정보 내용은 바꾸지 않는다. API와 E2E 번호를 유지하고 착지 창을 기다린다.

## 같은 웹 출력의 Rust 재컴파일 방지(t-0933)

- `npm run build`는 TypeScript 검사 뒤 Vite를 `web/node_modules/.cache/aaa-dist-*`에 실행한다.
  plugin 출력까지 완성한 다음 bytes가 다른 파일·추가·삭제만 `dist`에 반영한다. 같은 파일과
  상위 폴더의 mtime은 유지한다. Cargo의 기존 `web/dist` 감시는 보존하여 직접 고친 dist도 감지한다.
- `node --test web/test/build.test.mjs`는 실제 Vite의 같은 출력, JS 변경에 따른 자산 이름 교체,
  public 파일 추가·삭제, 실패 때 이전 dist·성공 표지 보존을 검사한다. 동기화 시험은 같은 길이·mtime의
  다른 bytes, 파일/폴더 교체, 대소문자만 다른 이름을 확인한다. 과거 시각을 직접 지정하여 고정 대기 없이 검출한다.
- 성공 시각은 감시 폴더 밖 `node_modules/.cache/aaa-web-build.stamp`에 기록한다.
  `ci-local --only e2e`는 표지와 소스·public·빌드 스크립트의 시각을 비교하고, 표지가 없으면 index를 쓴다.
  `scripts.test_ci_local`은 오래된 index와 최신 성공 표지, 표지 이후 스크립트 변경을 실제 CI 셸로 검사한다.
- 다른 Cargo 실행이 끝난 자기 worktree에서 `nice -n 15 python3 scripts/check_web_rebuild.py --logs target/web-rebuild-check`를 실행한다.
  실제 npm 빌드와 Cargo JSON의 `aaa-host`·`aaa` fresh를 대조한다. 동일 결과는 둘 다 fresh이며,
  시험 public 파일의 추가·같은 길이/mtime 변경·삭제는 둘 다 재컴파일해야 한다. 기존 host 자산 시험으로
  압축 복원한 내장 bytes와 전체 파일 목록을 원문과 대조한다. 시험 파일은 끝에 지우며 실패 단계·종료 코드·로그를 보존한다.
- 성능은 같은 SHA·도구·환경·worktree 캐시에서 원본과 변경판의 전체 `ci-local.sh`를 연속 실행하여 비교한다.
  준비 실행과 변경 없는 반복을 나누고 `workspace-summary.tsv`의 전체·단계 시간을 기록한다.
  Windows는 BelowNormal 프로세스에서 기존 release 선검사를 포함한다. 준비 실패는 성공 표본으로 세지 않는다.
- 먼저 [Cargo의 mtime 감시](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed)
  ([MIT 선택](https://github.com/rust-lang/cargo/blob/master/LICENSE-MIT)),
  [Vite build API](https://vite.dev/guide/api-javascript.html#build)
  ([MIT](https://github.com/vitejs/vite/blob/main/LICENSE)),
  [include_dir의 파일 포함·경로 추적](https://github.com/Michael-F-Bryan/include_dir/blob/master/macros/src/lib.rs)
  ([MIT](https://github.com/Michael-F-Bryan/include_dir/blob/master/LICENSE))를 대조했다.
  Vite의 완성된 출력과 Cargo의 기존 감시 경계를 따르며 외부 코드·의존성은 추가하지 않는다.
  release 병렬화·LTO·codegen-units와 API·E2E 번호는 바꾸지 않는다.
- 첫 변경판 전체 CI는 E2E12의 파일 보기 Back에서 30초 navigation 만료였다. 보고 iframe의 첫 HTML 응답을
  파일 보기 open 뒤에 전달한 진단은 주 문서 이동 없이 iframe만 이동하고 파일 보기가 남는 같은 만료를 재현했다.
  HTML 응답·그림만 보류한 다른 대조와 구별한다. E2E12는 기존 결과물 HTML·그림·sandbox 단언을 파일 링크
  검사 앞으로 옮겨 결과물 준비 뒤 파일 보기의 기록을 검사한다. Back·닫힘·모든 기존 내용/격리 단언과 시한은 유지한다.
  원본 실패 당시 iframe commit 기록은 없으므로 당시 사건 순서를 소급 확정하지 않는다. 제품 history 코드는 바꾸지 않는다.

## 자동화 기억의 압축·재개 재독(t-0941)

- `cargo test --locked -p aaa --test m13 automation_memory_reentry_`는
  격리 host·실제 agent·PTY의 가짜 Claude/Codex에서 `reuse=open`, `memory=true` 자동화를 띄운다.
  최초 기억을 옮긴 뒤 thread의 기억 파일을 바꾸고 `SessionStart(compact)`·`resume`을 CLI로 보낸다.
  resume 전에는 agent를 실제로 재연결하여 세션 명세의 재독 문맥이 유지되는지도 본다.
  가짜 harness는 hook 응답의 절대 경로를 열어 최신 bytes를 기록한다. 기록 파일 생성이 완료 사건이며
  하위 agent 사건은 응답이 비어야 한다. 재독 안내를 제거한 변형은 두 harness 모두 compact 뒤
  기억이 null이어서 실패한다. 실제 모델의 지시 이행·유료 모델의 실제 압축을 대신 검증하지 않는다.
- host의 `reuse_accepts_codex_and_seeds_memory_reentry_only_when_enabled`는 Codex 승인·기억 선전송·두 번째 입력의
  같은 thread 재사용과 `memory=false` 안내 부재를 검사한다. Codex 거부가 남은 원본에서 실패를 확인한다.
  CLI `codex_hook_cli_reaches_host_and_returns_guidance`는 일반 세션 안내·내구 명세의 기억 안내·압축·재개·하위 agent 제외를 검사한다.
  hook 유닛 시험은 진행 안내와 기억 안내가 SessionStart에서 함께 오고 다른 사건에 기억 안내가 없는지 검사한다.
- 참고 후보는 [Codex hooks](https://learn.chatgpt.com/docs/hooks#sessionstart)([Codex Apache-2.0](https://github.com/openai/codex/blob/main/LICENSE))와
  [herdr Codex hook](https://github.com/herdrdev/herdr/blob/v0.9.5/src/integration/assets/codex/herdr-agent-state.sh)([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE))다.
  SessionStart의 추가 문맥과 세션 귀속 경계를 따른다. [Claude SessionStart](https://code.claude.com/docs/en/hooks#sessionstart)도 대조했다.
  Claude Code 저장소는 상용 약관 적용으로 OSS로 간주하지 않는다. 외부 코드·의존성은 추가하지 않는다.
- 전체 로컬 ci-local·Linux strict cross·의존 경계·공개 검사가 정본이다. 고정 sleep·실패 재시도·시한 확대는 없다.

## E2E93 재개 뒤 출력 구독과 종료 입력(t-0927)

- t-0926 원본의 quit 5바이트는 host→agent→session PTY 쓰기 완료였고, 14:06:48.410Z에 agent의 `released` 감지도 있었다. 그러나 재연결 직후 보낸 두 session subscribe가 14:07:00.690Z에 모두 15초로 만료됐다. 화면은 `app-history-4`에 멈췄다. 부하 17.00/8.08/7.91을 시한 연장의 근거로 쓰지 않는다. 원본 보고·두 로그·화면/DOM/network 진단·검증 archive의 모든 파일을 읽고 사본을 보존했다. 당시 잠금 획득 순서는 로그에 없어 소급 확정하지 않으며, 아래 keeper에서 원본과 일치하는 구독 응답 유실 경로를 입증한다.
- 제품 원인: Welcome 뒤 `initialize`는 `harness_delivery` 잠금을 얻은 다음 host 송신자를 등록한다. 복구한 감지·hook 저장이 이 잠금을 쥐는 동안 reader는 초기 Subscribe를 처리할 수 있지만, session pump의 `send_host`는 송신자가 없어 응답을 버린다. 실제 두 session pump와 frame 연결을 사용한 `link::resume_tests::requests_read_before_host_publication_keep_session_replies_and_exit_output`은 잠금을 쥔 채 Welcome·두 요청·Ping을 읽게 고정한다. 수정 전에는 Pong 뒤에도 두 구독 응답이 없어 실패했다.
- 요청 task는 host 등록 사건을 기다린 뒤 시작하며 reader의 Ping·EOF 처리는 계속된다. gate는 전체 replay 완료보다 먼저 열려 기존 replay 중 응답 동시 처리와 live hook 순서를 유지한다. keeper는 두 구독 응답→quit ACK→종료 출력 bytes를 검사한다. `eof_or_deadline_before_host_publication_does_not_start_mutating_requests`는 등록 전 EOF·요청 자체의 시한 만료 때 이름·spec 파일을 바꾸지 않는지 검사한다. 등록 대기에 쓴 시간도 기존 요청 예산에서 빼며 timeout을 늘리지 않는다. 기존 replay·backpressure·연결/codec 시험도 유지한다. wire/API·시한은 변경하지 않는다.
- 별도로 기존 `fake-shell-harness.py --mouse`는 ESC 입력을 SGR 휠 64/65 정규식이 맞을 때만 비웠다. VT200 클릭(`CSI <0;…M/m`) 뒤 휠·quit까지 삼키는 결함을 실제 PTY와 WebKit 390의 실제 `.xterm-screen` 클릭으로 재현했다. 이 대조도 `resume:quit` 20초 실패였지만, 원본의 released·subscribe 만료와 구별한다. 시험 harness는 명령/제어 버퍼를 나누고 완성 CSI를 소비하며 휠 누름만 화면을 굴린다.
- 기존 재개 8개 조합에서 실제 클릭→대화→agent 재시작→quit 경로와 클릭 누름·뗌 수신·quit 해석·최종 출력/정리를 검사한다. 직접 실행 8개와 기존 휠·기록·보기 복원도 유지한다. 최근 64개 입력 종류·button/final·명령 분류·종료 단계만 실험실에 기록하며 원문 글·좌표·환경값은 기록하지 않는다. 실패 시 `diagnostics/<조합>/`에 기존 자료와 pane 상태·최근 화면·load·해당 session-host 로그·입력 분류를 둔다. 바깥 단계 시한에도 수집하며 진단 오류가 최초 시험 오류를 덮지 않는다.
- 참고 후보는 [herdr-web-ui PaneTerminal](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/PaneTerminal.tsx)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE))과 [xterm 5.5 CoreMouseService](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/common/services/CoreMouseService.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))다. 전자의 입력 전달 경계와 후자의 VT200 클릭·휠 보고를 대조했다. 외부 코드·의존성은 추가하지 않는다.
- 로컬 정본은 Playwright 1.64.1의 전체 E2E93, 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사·Linux strict cross clippy다. 기존 DOM 20초·locator 30초·단계 120초, 단언·E2E 번호를 유지하며 고정 sleep·실패 재시도·시한 연장은 없다. t-0928과 겹칠 수 있는 곳은 E2E93 spec·시험용 Python harness·이 문서 절이다. 공통 runner·시한 정책은 변경하지 않는다.

## 휴대폰 대화의 할 일 링크 뒤로·앞으로(t-0924)

- `node web/e2e/web.e2e.mjs --spec 77 --workers 1`: 실제 host·agent의 coordinator 대화에서 d-ID 링크를 누른다. Chromium·WebKit 390px에서 native `history.back()`의 popstate와 React 반영 frame을 기다려 원래 대화 주소·대화 본문·상세 제거를 검사한다. 앞으로는 같은 할 일 상세·강조가 돌아오고 시트 표지가 없어야 한다. 두 번 왕복 뒤 Chromium의 실제 기록 목록/CDP 위치와 WebKit 기록 길이가 처음 상세를 연 직후와 같아야 한다.
- 수정 전 두 엔진에서 뒤로 한 번에 대화 대신 `#tasks`가 되는 실패를 재현했다. 깊은 주소의 기록 위에 휴대폰 시트 기록이 하나 더 생기고, 뒤로 시트의 닫기 콜백이 새 목록 주소를 쌓는 원인이다. 깊은 주소로 연 상세의 추가 시트 기록만 끈다. 기존 E2E77의 첫 주소·새로고침·프로젝트 한정 주소·삭제/보관·닫기·Reader·보고서 링크 검사를 유지한다.
- 완료 사건은 popstate·DOM·animation frame이다. 고정 sleep·재시도·시한 변경은 없으며 headless WebKit의 기록 이동을 실제 iOS 몸짓 애니메이션 검증과 동일시하지 않는다. 공통 `sheetHistory.ts`·설정 drawer·E2E39는 수정하지 않는다. t-0923의 E2E39 간헐 실패 수리와 파일이 겹치지 않는다.
- 참고 후보는 [herdr-web-ui 시트 기록](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/sheetHistory.ts)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE))와 [remix-run/history](https://github.com/remix-run/history/blob/main/packages/history/index.ts)([MIT](https://github.com/remix-run/history/blob/main/LICENSE))다. 전자의 시트 기록과 후자의 PUSH/POP 구분을 참고했고 외부 코드·의존성을 추가하지 않는다. [History API의 기록 추가](https://developer.mozilla.org/en-US/docs/Web/API/History/pushState)와 [WebKit의 뒤로·앞으로 몸짓](https://developer.apple.com/documentation/webkit/wkwebview/allowsbackforwardnavigationgestures)도 확인한다.
- 정본은 기준 main `5085e64f` 위 전체 `nice -n 15 bash scripts/ci-local.sh`·Linux strict cross clippy·공개 검사다. GitHub CI는 기다리지 않고 착지 창 전에는 main 병합·push하지 않는다. API와 E2E 번호는 유지한다.

## 간헐 E2E·Rust 시험의 회귀 소유 정리(t-0928)

실패 재시도·시한 확대·기대값 완화 없이 제품 결함과 시험 경합을 나눈다. 과거 보고에서 E2E66은 부하 중 최소 3회,
E2E11·52·89와 아래 m13 세 경로는 각각 최소 1회 실패를 확인했다. 전체 실행 횟수가 없어 실패율로 해석하지 않는다.

| 시험 | 수정·축소 이유 | 유지하거나 옮긴 회귀 단언 |
| --- | --- | --- |
| E2E66 줄바꿈 | 168음절 전수 조합·수백 키 입력을 native 줄 경계 전후 10표본으로 축소. 시험 helper의 캐시와 같은 helper의 fresh 자기 비교 삭제 | Chromium·WebKit × 데스크톱·휴대폰, 독립 native textarea의 줄 수·폭, IME 중/확정, 긴 입력·영문·paste·resize·높이 복원·focus·미전송 |
| E2E11 선택 질문 | PTY 출력 뒤에도 host 답 저장 응답이 남을 수 있음 | Enter 전에 등록한 `question_answer` 응답 완료·오류 검사 뒤 정확한 두 답과 화면 입력을 대조 |
| E2E52 첨부 | 실제 저장 응답을 늦추면 다른 task 선택을 덮는 제품 결함 | 실제 `task_edit`를 처리한 뒤 응답만 gate로 보류. 다른 task 선택 뒤 풀어도 선택 보존. 기존 파일 bytes·메모·첨부·떼기 단언 유지 |
| E2E89 책임 경고 | 일반 HTTP 전부의 fetch/fulfill 고리에서 이미 처리한 Route 오류 | context의 실제 `Response.json()` 결과 중 책임 관측값만 fixture로 변환. HTTP 수명은 브라우저에 맡기고 상태·Reader·폭·무변경 단언 유지 |
| m13 `remote_dir_thread_survives_host_restart` | 물리적 재접속을 outage 5초와 경쟁시키는 단언·7초 sleep·추가 재시작 두 번 제거 | 실제 TLS 재연결·보고 수집·사건은 m13. 시간 경계·down/recovered 중복·실제 DB 재개는 `inbox::tests::restarted_host_recovers_only_an_outage_still_open` |
| m13 `memory_and_restart` | 짧은 working 유예를 CLI 왕복으로 확인하는 중복 단언 제거 | 유예 경계는 `orch::tests::a_just_delivered_brief_is_working_until_a_report_or_a_tick`. 실제 memory 변경·새 pane·brief·resume와 종료 pane idle은 m13에 유지 |
| m13 `claude_thread_states_questions_and_messages` | accepted와 harness 실제 수신을 구분. 제한된 visible 화면에 과거 출력이 계속 남을 필요 없음 | 실제 `got` 출력 뒤 interrupt/stop. Interrupted→긴급 입력 순서·중단 횟수·권한 질문 보호·정확한 질문/입력 기록 유지 |

- memory 재시작은 새 원문 `API는 v3로 바꾼다`의 포함과 옛 원문 `API는 v2로 한다`의 부재를 함께 검사한다. brief의 진행 보고 실행 경로에 우연히 `v2`가 있어도 실패하지 않는다. `landing-v2` 경로에서 실제 시험을 수행하고, 새 memory에 옛 원문도 함께 남긴 변형이 같은 단언에서 실패하는지 확인한다.

- E2E52의 정상 저장은 그대로 새 폼을 연다. 응답을 기다리던 폼의 선택이 현재 선택과 같을 때만 새 폼을 열어 다른 선택·닫힘·초안을 덮지 않는다. 구판에 응답 gate를 적용하면 선택 보존 단언이 실패하고 수정판은 기존 전체 첨부 흐름을 통과한다.
- 보고가 brief 확인보다 먼저 도착한 종료 pane은 최근 전달 유예 때문에 working에 남았다. `just_delivered`에 살아 있는 pane 조건을 추가하고 기존 `a_closed_pane_needs_you_only_without_a_report`에 같은 기록의 실행/종료 비교를 넣는다. 구판의 결정적 RED와 수정판 GREEN을 확인한다.
- outage 기존 시각 경계에 기준 직전/정각, 실제 `Host::open`으로 DB를 다시 연 뒤 down 중복 없음·회복 한 번·새 host의 짧은 끊김을 더한다. 제품의 outage 기준을 0으로 바꾸면 기준 직전 단언이 실패한다. 줄 수를 항상 1로 망가뜨리면 축소한 E2E66이 네 화면의 줄 분류에서 실패한다. 변형 뒤 원본을 복원하고 웹·실행 파일을 다시 빌드한다.
- 고정 sleep ratchet에서 m13의 제거된 1건을 내린다. 새 시험 전용 production 고리는 없다. E2E39·03·14·26·97의 기존 수리는 보존한다. E2E93 재개의 Subscribe 응답 유실과 가짜 터미널 제어 입력 파서는 t-0927이 맡는다. E2E47 닫기 실패는 즉시 Escape 대조로 재현하지 못했으므로 근거 없이 고치지 않는다.
- 정본은 `nice -n 15 bash scripts/ci-local.sh`·Linux strict cross·공개 검사다. 고정한 `AAA_BIN`으로 전체 E2E를 `--repeat 2` 실행해 동시 Rust/E2E 부하·작업별 시간·실패 원문을 기록한다. 이 반복은 측정이며 실패를 통과로 치환하지 않는다. GitHub CI는 기다리지 않는다.
- 추가 E2E 6 workers를 기본 CI와 겹친 과부하 측정은 E2E 정본 126/126, 반복 250/252였다. 반복의 E2E07 설정 클릭·E2E12 Markdown 보기 실패는 원본 화면 상태가 부족해 원인 미확정으로 남긴다. 해당 단언에 단추 상태·애니메이션·보기 오류·자료 요청 시간 진단을 추가한다. Rust SSH 감지는 내구 DB에서 시한 오류를 회수해 단언에도 그 이유를 넣고, restart 준비 실패는 실제 sample의 dyld 초기화와 기한 뒤 연결 기록을 보존한다. 시한·핵심 단언은 그대로이며 이 과부하 실패를 이후 기본 CI 통과로 지우지 않는다.
- 참고는 [Playwright의 사용자 동작·격리·대기](https://playwright.dev/docs/best-practices)와 [flaky 구분](https://playwright.dev/docs/test-retries)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [herdr-web-ui의 unit/integration 분리](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/scripts/ci-tests.ts)와 [독립 UI 세션 정리](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/scripts/ui-contexts-regression.ts)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE))다. 계층별 회귀 소유·완료 사건·격리를 참고했으며 코드·의존성을 가져오지 않는다.

## E2E39 설정 준비와 첫 thread 목록의 경합(t-0923)

- `pairPage`의 머신 행 부착은 첫 thread 목록 수신·자동 첫 선택 완료가 아니다. 실제 첫 `threads` 응답을 메뉴 열림 뒤까지 보류하면 빈 주소의 자동 `choose`가 메뉴를 닫아 `pane-bk1` 클릭이 viewport 밖→비표시로 30초에 실패한다. t-0921 원문과 같은 실패 경로를 재현했지만, 원문에는 엔진·서랍 상태가 없어 당시 사건 순서를 소급 확정하지 않는다.
- 설정 owner는 시작 주소를 기존 `paneHash("bk1")`로 명시한다. 실제 pane 클릭과 설정 목록·모양 깊이 2, 패널/X 닫기, 늦은 내부 되감기 뒤 메뉴 유지, bk2 선택과 사용자 Back/Forward의 메뉴 닫힘 검사는 유지한다. 앱의 history·공통 `inList`·시한·기대값은 바꾸지 않는다.
- E2E39의 「설정 준비」는 Chromium·WebKit 390px에서 실제 host의 첫 `threads` 응답을 메뉴가 열린 사건 뒤에 전달하고 bk1 클릭 완료를 검사한다. t-1153의 초기 표시 계약에서는 첫 전체 목록 전 pane 행이 없으므로, 이 구간의 로딩/행 숨김을 확인하고 메뉴만 연다. 응답 전달과 `threadsSync`의 전체 목록 반영·pane 행을 기다린 뒤 메뉴 유지를 단언하고 실제 bk1 줄을 직접 누른다. `inList`로 메뉴를 다시 열어 자동 첫 선택의 닫기 회귀를 숨기지 않는다. context.route가 요청을 소유하도록 이 gate 대조만 Service Worker를 막는다. 일반 설정 owner는 원래 Service Worker 설정을 쓴다. 초기 주소 지정을 제거한 변형은 같은 keeper에서 실패해야 한다. 고정 sleep·실패 재시도·상한 연장은 없다.
- 실패 시 `diagnostics/settings-drawer/<engine>-<close>.json`·PNG에 엔진·panel/X/준비 경로·진행 단계, 원래 오류·stack, drawer/pane/toggle/panel의 표시·좌표·transform·animation, 설정 기록과 되감기, 최근 클릭·기록 이동·서랍 변화 사건을 남긴다. context 정리와 화면을 움직이는 공통 진단보다 먼저 저장하며, 바깥 단계 120초 만료에도 같은 `diagnose`를 호출한다. pairing query·cookie·임의 history.state·본문은 수집하지 않는다. 저장 오류는 최초 시험 오류를 덮지 않는다.
- 참고 후보는 [Playwright actionability](https://playwright.dev/docs/actionability)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와 [Cypress 요소 조작](https://docs.cypress.io/app/core-concepts/interacting-with-elements)([MIT](https://github.com/cypress-io/cypress/blob/develop/LICENSE))다. visible과 실제 클릭 가능·animation·viewport를 구별하며 `force`·DOM click으로 우회하지 않는다. 외부 코드·의존성을 추가하지 않는다.
- 로컬 검증: Playwright 1.64.1의 전체 E2E39, 준비/설정 owner와 초기 주소 제거 RED 대조, 웹 유닛·빌드·공개 검사. t-0924의 할 일 시트 history 수리와 변경 파일이 겹치지 않는다. 이 시험은 실제 iPhone OS 몸짓을 검증한 것이 아니다.

## Windows bootstrap·암호 console의 자식 환경 격리(t-0922)

- `cargo test --locked -p aaa --test windows_bootstrap`의 owner는 첫 cmdlet 전에
  `$PSHOME/Modules`만 검색한다. shell별 새 분석 캐시와 `Get-FileHash`·`Invoke-RestMethod`를
  가로채는 외부 시험 모듈 경로를 상속시켜도 실제 PS5.1/7의 내장 명령으로 기존
  PE·SHA·native argv·종료 코드·표준 입출력·기존 파일 보존·실제 IRM/IEX 입구를 검사한다.
  PS7 → Rust → PS5.1처럼 중간 native 프로세스가 있으면 PS7의 자동 모듈 경로 정리가
  적용되지 않는 경계를 다룬다. 생산 bootstrap과 사용자·시스템 모듈 설정은 바꾸지 않는다.
- `cargo test --locked -p aaa --bin aaa install::engine::input::password::tests::console_masks_and_restores_after_success_cancel_and_eof -- --exact --nocapture`는
  ConPTY 자식에서만 `AAA_SESSION_ID`·`AAA_HOOK_SOCK`를 제거한다. 시험 runner가 aaa 세션
  안이어도 성공·Ctrl-C·EOF의 보호 입력·비 echo·mode 복원을 검사한다. 두 변수를 각각
  다시 넣은 자식은 입력 prompt 없이 원래 세션 거부 오류로 끝나야 한다. 실제 계정 암호는 쓰지 않는다.
- 참고 후보는 [PowerShell의 모듈 경로 구성](https://github.com/PowerShell/PowerShell/blob/master/src/System.Management.Automation/engine/Modules/ModuleIntrinsics.cs)([MIT](https://github.com/PowerShell/PowerShell/blob/master/LICENSE.txt)),
  [Cargo의 시험 자식 환경 격리](https://github.com/rust-lang/cargo/blob/master/crates/cargo-test-support/src/lib.rs)([MIT 선택](https://github.com/rust-lang/cargo/blob/master/LICENSE-MIT)),
  [Pester의 명시적 시험 모듈 import](https://github.com/pester/Pester/blob/main/test.ps1)([Apache-2.0](https://github.com/pester/Pester/blob/main/LICENSE))다.
  [공식 PSModulePath 설명](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath#starting-windows-powershell-from-powershell-7)에
  따라 자식 환경 경계를 명시하며 외부 코드·의존성은 가져오지 않는다.
- Windows 실기는 전용 실험실에서 PS7의 모듈 경로와 가짜 세션 변수 둘을 가진 runner로
  두 owner를 실행한다. 전역 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준 목록을
  전후 대조하고 시험 폴더를 정리한다. 로컬 전체 ci-local·공개 검사를 함께 실행하며
  GitHub CI는 기다리지 않는다. 기존 시한·기대값을 완화하거나 실패를 재시도하지 않는다.

## 하위 오류 진단과 비밀값 경계(t-0912)

- `cargo test --locked -p aaa-common --lib diagnostic::`: 중첩 anyhow의 OS 오류가 응답 왕복과 실제 tracing 사건에 남는지 검사한다. serde 오류 자체에 가짜 토큰이 들어가는 data 오류를 주입해 분류·행·열만 남는지, 닫힌 oneshot의 payload가 로그로 나오지 않는지도 검사한다. subscriber는 시험 thread 안에서만 설치한다.
- `node --test web/test/apiOutcome.test.mjs web/test/errorDiagnostic.test.mjs`: 서버의 원인 요약 유지, 비 JSON·잘못된 envelope·HTTP 실패, 읽기 중 끊긴 body를 검증한다. 원문 오류/HTML을 표시하지 않는 단언도 포함한다.
- 첫 전체 검증의 E2E88은 WebKit 데스크톱에서 B 선택이 모두 false였다. 별도 격리 진단에서 영상 응답을 pointerdown 뒤까지 보류한 뒤 metadata를 로드하면 선택지 y가 583.72→738.03으로 이동하고 pointerup은 A 영상에 도착했다. 원본의 button 없는 click과 같은 형태이며, 원본 시각의 pointerdown 자료는 없어 그 실행의 상세 순서를 소급 확정하지 않는다. 시험은 세 영상의 metadata 사건을 모두 본 뒤 B를 고르고, 재생 전 B 선택 반영과 기존 재생 후 B 유지 단언을 함께 검사한다. 원래 15초·20초 상한·양 엔진·양 화면·popup·sandbox·Range 검사를 유지한다. 서비스 워커 차단/응답 gate는 원인 진단에만 썼다.
- 보조 정리·선택 파일의 fallback은 그대로이며 debug 관측만 더한다. 각 호출과 같은 모양의 시험을 늘리지 않고, 위 공통 소유 경계와 기존 host·agent·설치·웹 회귀를 전체 ci-local로 검사한다. Windows·Linux 전체 cross clippy와 Windows의 공통/설치 오류 시험도 실행한다. 고정 sleep·실패 재시도·시한 변경은 없다.
- 오류 전달·민감 자료를 다루는 방식과 라이선스 참고는 [하위 오류 진단](error-diagnostics.md)에 적었다.

## Windows SCM rollout과 플랫폼별 후보(t-0907)

- host `rollout::web_tests::mixed_platform_candidates_are_pinned_before_transfer_and_guard_uses_target_sha`는
  macOS·Windows hello를 두고 빠진 플랫폼 후보 거부, 단계별 후보 고정, 재연결 뒤 선택 보존,
  전송 영수증·guard의 Windows SHA 대조를 확인한다. 기존 rollout·복구·권한 시험도 유지한다.
- agent `deploy::guard::tests::windows_restart_is_owned_by_the_guard_and_is_not_repeated_after_resume`는
  활성화 전 미실행, 활성화 뒤 한 번, 내구 기록 재독 뒤 재실행 없음과 기존 복구 전이를 검사한다.
  `setup::tests::rollout_scm_restart_checks_ownership_and_runs_stop_then_start_once`는 가짜 Windows System으로
  자기 서비스·다른 명령 거부, 정확한 시험 instance 이름, stop→start와 시작 실패의 재시도 없음을 확인한다.
- guard의 차단된 서비스 작업 시험은 게이트를 닫은 채 동시 Status 세 개·Observe·Commit을 실제 IPC로 처리한다.
  확정 뒤 작업 게이트를 열어 SCM 오류가 늦게 도착해도 확정 상태를 되돌리지 않는다. 이미 복구 중인 시한도 늘리지 않는다.
- Windows 실기는 `scripts/test_windows_rollout.py`를 명시해서 실행한다. macOS의 별도 host·local agent,
  암호 없는 SCM 가상 계정의 `aaa-test-NNNN-x-agent`, SSH 역방향 시험 포트만 사용한다.
  실제 두 OS 후보를 같은 커밋 salt로 빌드해 준다. 시험 도구는 macOS launchctl 요청만 기록하고,
  자신이 소유한 host·agent PID를 교체해 실제 재연결을 관측한다. Windows 교체와 SCM 중지·시작은 제품이 수행한다.
  성공의 대상별 실행 SHA·안정된 작업 ID·done을 확인한다.
- `--failure-binary`에는 `--version`·`config check`를 통과하고 서비스 입구는 실패하는 시험 PE를 준다.
  두 번째 rollout의 Windows 단절 직후 시험 host를 내린다. host가 없는 동안 Windows 서비스가
  baseline 파일 SHA와 SCM Running으로 복구돼야 한다. host를 다시 띄운 뒤 같은 작업 ID의
  `stopped_rolled_back`·guard `restored`, 앞서 성공한 macOS 파일 보존을 확인한다.
  후보 runtime 실패를 만들려고 제품에 시험용 분기나 시한 변경을 넣지 않는다.
- 도구의 Windows 경로는 `C:\aaa-test\t-NNNN`만 받고 instance는 `aaa-test-NNNN-x` 형식으로 제한한다.
  전역 PATH·사용자 서비스·sshd·방화벽·GUI는 바꾸지 않는다. 시험 전 서비스·예약 작업·Run 키·사용자 PATH·
  방화벽 목록을 보존하고, 도구 종료 뒤 시험 폴더·프로세스·서비스를 정리한 다음 기준과 대조한다.
  guard PID는 해당 실험실의 실행 경로·명령줄을 확인한 것만 각각 종료한다.
- 첫 실기의 감시 준비 오류 5는 Windows `File::open(...).sync_all()`을 별도 네이티브 대조로 확인했다.
  읽기 전용 핸들은 오류 5이고 쓰기 핸들은 성공한다. 복구 사본은 쓰기 핸들로 flush하며
  복사·동기화·감시 준비에 실패하면 설치를 시작하지 않는다. 실패를 재시도나 시한 연장으로 넘기지 않는다.
- 정본은 전체 `nice -n 15 bash scripts/ci-local.sh`(Playwright 1.64.1), Linux strict cross clippy,
  Windows 네이티브 build·clippy·관련 owner와 공개 검사다. 외부 Windows 실기는 전체 ci-local에서 자동 실행하지 않는다.

## 이미지 결과의 base64 글 가림 충돌(t-0918)

- 기존 m13 이미지 owner의 가짜 Codex는 표준 라이브러리만으로 887×1774 RGBA·544 KiB PNG를 만든다. CRC가 있는 ancillary chunk에 가짜 인증과 인코딩 글자만 겹치는 bytes를 넣는다. 로컬·TLS 원격 thread의 profile env에 같은 가짜 인증을 등록하고 실제 CLI→hook→agent link→host→가짜 생성→조각 전송을 탄다. 생성 계정·외부 API는 쓰지 않는다.
- 수정 전 실제 `generate`는 `Invalid symbol 42, offset 3787`로 실패했다. 수정 뒤 로컬·원격 생성, 같은 ID 재전송·`fetch`, host 재시작·새 pane의 기존 결과 수신, SHA·길이 검증과 기존 인증 비노출·접근 거부·중복 생성 방지 단언을 유지한다. PNG에는 첫 조각 외에 64 KiB 경계 양쪽과 마지막 조각에도 충돌을 넣는다.
- `hooks::tests::image_reply_preserves_encoded_collisions_and_masks_actual_secret_bytes`는 64 KiB 전후 세 길이에서 옛 글 가림의 정확한 3787 decode 오류와 새 응답의 bytes 일치·EOF를 대조한다. 실제 인증 bytes·일반 hook 글·hook 실패·이미지 오류는 계속 가려야 한다. 기존 session-start 안내 시험도 타입 있는 hook 응답을 확인한다.
- 운영 두 PNG의 읽기 전용 대조에서 offset 3787 주변 base64 구간이 같고 대응 raw 위치가 모두 첫 `caBX` chunk 안임을 확인했다. 이 chunk는 [C2PA PNG의 JUMBF 컨테이너](https://spec.c2pa.org/specifications/specifications/2.2/specs/C2PA_Specification.html#_special_consideration_for_png)다. 같은 오류 위치가 전송 청크 경계임을 뜻하지 않는다. 실제 인증·인증서 본문이나 지문은 시험 자료로 복사하지 않는다.
- 먼저 [Serde serializer의 newtype 구별](https://github.com/serde-rs/serde/blob/master/serde_core/src/ser/mod.rs)([MIT](https://github.com/serde-rs/serde/blob/master/LICENSE-MIT))과 [tracing의 구조 있는 진단](https://github.com/tokio-rs/tracing/blob/master/tracing/README.md)([MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE))을 조사했다. 기존 B64 가림 serializer를 IPC 직렬화까지 적용하고 단계·작업·offset 문맥을 유지한다. Relay의 타입별 PII 처리도 대조했으나 FSL-1.1-Apache-2.0이라 코드를 가져오지 않았다. 새 의존성은 없다.
- 정본은 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사다. 사건·프로세스 종료·원본 SHA로 끝내며 sleep·재시도·시한 연장·기대값 완화는 없다. wire·기능 협상·FRONT_API·AGENT_LINK·E2E 번호를 바꾸지 않는다.
- t-0918의 전체 실행(622초)은 web·fmt·build·clippy·public 통과, Rust 1,227개 통과·1개 실패, 웹 작업 124/125개 통과였다. load average 87.53에서 원격 재시작 시험은 listen 뒤 재접속 5.50초로 outage 기준 5초를 넘었고, E2E66의 네 화면 합계는 127.97초로 단계 120초를 넘었다. 원본을 보존하고 다른 검증을 마친 뒤 같은 실행 파일·단언·시한으로 각각 한 번 분리 대조했다. 원격 시험은 17.01초, E2E66의 같은 줄바꿈 단계는 20.54초(전체 파일 30.75초)에 통과했다. 전체 명령의 실패 결과를 성공으로 바꾸지 않는다.

## Windows 한 줄 설치 owner의 첫 모듈 탐색 격리(t-0904)

- 원본 CI39935743297은 PS5.1 `irm_entry`·HTTP accept 대기(1)·질문 0/16이었다. 원본에는 명령 trace가 없으므로 당시 멈춘 메서드를 소급 확정하지 않는다. 별도 Windows 진단39942924094의 차가운 첫 `irm`은 AWS·Graph 등 러너의 무관한 모듈을 분석했다. AWS 분석·캐시 처리의 한 구간만 35.79초였고, 실제 Utility 모듈을 찾기까지 53.46초가 걸렸다. 이 대조는 HTTP(4)·`payload_written`까지 간 뒤 원래 60초에 실패했으며, 종료 직전 PowerShell 스택은 설치 자식의 `Process.WaitForExit`였다. HTTP 이전의 환경 준비가 설치 owner의 사건 상한을 소모하는 경로를 확인한 것이다.
- `recovery::one_line_entry_keeps_the_external_console_through_review_and_signed_file_installation`은 첫 cmdlet 전에 해당 인터프리터의 `$PSHOME/Modules`만 검색하게 한다. `PSModuleAnalysisCachePath`도 shell별 Lab 파일로 지정한다. Windows 폴더 API는 LOCALAPPDATA 환경 변경만으로 다른 캐시 폴더를 반환하지 않으므로 기존 환경 격리만으로는 충분하지 않았다. 사용자·시스템 모듈이나 캐시 설정을 바꾸지 않는 시험 전용 변경이다. 생산 설치기·실제 `irm | iex`·공통 PTY 60초 상한은 그대로다.
- keeper는 새 캐시와 함께 `Invoke-RestMethod`를 내보내고 즉시 오류를 내는 외부 시험 모듈을 상속 경로에 넣는다. 실제 PS5.1/7의 내장 IRM으로 HTTP 본문을 받고, 기존 16개 질문·review/yes·서명 승인·파일/status·옛 실행 파일 보존을 모두 검사한다. 모듈 경로 격리를 지운 대조는 외부 명령을 선택해 이 실제 입구에서 실패해야 한다. 캐시 예열·고정 대기·시한 연장·재시도로 통과시키지 않는다.
- 참고 후보는 [PowerShell 명령 자동 탐색](https://github.com/PowerShell/PowerShell/blob/v6.0.0/src/System.Management.Automation/engine/CommandDiscovery.cs)(MIT), [Pester CI의 PS5.1/7 분리](https://github.com/pester/Pester/blob/main/.github/workflows/ci.yml)와 [시험 모듈의 명시적 import](https://github.com/pester/Pester/blob/main/test.ps1)(Apache-2.0), [cargo-dist PowerShell 설치기](https://github.com/axodotdev/cargo-dist/blob/main/cargo-dist/templates/installer/installer.ps1.j2)(MIT 선택)다. Pester의 해당 경로에서 첫 사용 지연 전용 우회는 찾지 못했으며, 외부 코드·의존성은 가져오지 않았다. 캐시 위치 지정은 [PowerShell 5.1 공식 설명](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_windows_powershell_5.1?view=powershell-5.1#module-analysis-cache)을 따른다.
- 정본은 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사와 실제 Windows의 `cargo test --locked -p aaa --test install`이다. Windows owner는 macOS에서 cfg로 제외되므로 로컬 통과를 Windows 실행으로 대신하지 않는다.
## 웹 E2E 스크롤·닫힘 진단(t-0905)

- E2E93의 Chromium 휴대폰 준비는 터치 뒤 `scrollTop=0` 쓰기 대신 실제 반대 방향 터치로 처음까지 돌아가고 `scrollend`를 기다린다. [CI 39950463717](https://github.com/cLazyZombie/aaa/actions/runs/39950463717) 두 실패의 내부 trace는 최종 터치 위치 197.5/DOM 198 → 프로그램 쓰기 0 → compositor delta -1 → +1을 보인다. 새 입력·앱의 0→1 쓰기 없이 새 답변 전과 후에 각각 top 1이 됐다. 앱 없는 최소 HTML에서도 185.5px 터치 뒤 같은 프로그램 쓰기는 top 1, 실제 터치 복귀는 top 0이었다. 안내 높이 변화·실제 1px 이동과 읽던 줄 유지·새 답변 전후 정확한 0 단언은 그대로다. 브라우저 결함을 앱의 새 답변 갱신으로 오인하던 준비를 고치며, 제품에 1px 무시나 위치 재설정 루프를 넣지 않는다. 반올림 경로는 [Chromium ScrollTree::PullDeltaForMainThread](https://chromium.googlesource.com/chromium/src/+/156.0.8078.4/cc/trees/property_tree.cc)와 대조했다.

- E2E14는 실제 즉시 닫기·보내는 중 닫기를 유지하고, 클릭과 같은 task의 pagehide·beforeunload·hidden도 검사한다. 구판은 DOM 테마가 dark인데 실제 fetch는 0건이다. 종료 감시를 pending effect에서 분리하고 store의 현재 값을 읽는 판은 최신 dark를 keepalive로 정확히 한 번 보낸다. 종료 알림 두 번의 중복 방지와 host/다른 기기 반영을 함께 확인한다. 원본 CI에는 보내는 문서의 lifecycle 기록이 없어 당시 순서를 이 대조와 동일하다고 소급 확정하지 않는다.

- E2E03은 PageUp→늦은 native scroll→PageDown뿐 아니라 반대 방향도 고정한다. 구판은 맨 아래 동기화 뒤의 PageUp이 20초 동안 취소된 상태(native 3249, 첫 줄 s-172)로 남았다. xterm 처리 전부터 끝이었는지를 capture에서 기억해 글꼴 보정 범위를 좁힌 판은 Chromium·WebKit 양쪽을 통과한다. 글꼴 17→19px 전환 뒤 끝 유지(E2E07)도 별도로 확인한다. 키·native 사건·버퍼 값이나 기존 시한은 바꾸지 않는다.

- E2E03은 실패한 viewport가 닫히기 전에 실제 키 처리 여부·native scroll·위치 쓰기·렌더 grid를 남긴다. 앞서 공통 진단에는 이미 닫힌 실패 화면 대신 기본 pane만 남았다.
- E2E93은 터치·scrollend·위치 쓰기와 reset 전후 상태를 기존 대화 실패 자료에 함께 남긴다. E2E14는 닫는 문서의 lifecycle·keepalive 전송·요청 실패를 host 문서의 실패 진단과 함께 기록한다.
- 계측은 사건·값을 바꾸지 않는다. 기존 20초 상한과 정확한 위치·테마 단언을 유지한다.
- 별도 실제 Chromium 대조에서 `domIs`는 top=100·native scrollend 완료 후에도 끝나지 않았고, 위치가 같은 채 속성만 바꾸자 끝났다. scrollTop은 DOM mutation이 아니므로 `scroll`도 capture에서 관측하고 정리한다. 같은 대조의 수정판은 native scrollend 직후 끝난다. 이 누락을 원본 PageUp 실패의 원인으로 단정하지 않는다.
- 참고: ttyd의 연결별 reset·dispose, WeTTY의 터미널 write 완료 ACK, xterm.js 5.5의 DOM 위치와 비동기 scroll 동기화(MIT), Playwright 공식 사건 대기·trace 문서(Apache-2.0). 외부 코드는 복사하지 않는다.

## E2E93 재개 조합의 독립 실행과 실패 phase(t-0896)

- [CI 39923503523](https://github.com/cLazyZombie/aaa/actions/runs/39923503523)의 원본은 Claude 네 조합을 한 단계로 실행하다 합계 120초에 끝났다. artifact에는 e2e.log·실행 시간표만 있고 `resume-*.json`·host/agent 로그는 없다. 기존 `resumed` 안의 catch는 바깥 단계 타이머가 먼저 실패하면 실행되지 않는다. 당시 `--resume`와 `--continue` 중 어느 mode였는지는 원문으로 확정할 수 없다.
- 실패 화면의 390px 글꼴 직렬화는 두 엔진 대조에서 WebKit과 일치한다. 마지막 기록은 t1의 112×32와 harness의 47×35 snapshot이 번갈아 오고 입력·ACK도 계속된다. 1006만으로 같은 pane의 자동 재접속을 뜻하지 않는다. 로컬 phase·WebSocket 대상 계측은 정상 왕복에서도 같은 닫힘이 나오며, 연결 대상이 같은 연속 재접속은 0회였다. 원문에는 URL별 연결 ID가 없어 모든 닫힘의 원인을 소급 확정하지 않는다. E2E03의 기존 스크롤 수리와 같은 제품 원인이라고 묶지 않는다.
- Codex·Claude 직접 실행·재개는 Chromium·WebKit ×1280·390px마다 별도 단계(전체 16개)다. Claude의 resume→continue·이전 전체 기록·터미널 휠 두 번과 복귀·보기 선택·agent 재시작·PID 재연결·종료 검사는 한 조합 안에서 그대로 이어 간다. 병렬 입구는 E2E37의 기존 계획 경로로 E2E93 단계를 각각 새 host·agent·기기 실험실에 둬 다른 조합의 agent 재시작과 섞이지 않게 한다. 단일 worker는 한 실험실에서 원래 순서로 실행한다. 직접 실행의 기존 스크롤·새 기록·복귀·종료 단언도 유지한다.
- 기존 단계 120초·DOM 20초·locator 30초를 바꾸지 않는다. 독립 조합에 각자의 단계 한도를 적용하므로 네 조합 합계를 120초 성능 기준으로 재지는 않는다. 고정 sleep·실패 재시도·기대값 완화는 없다. `node --test web/test/e2e-jobs.test.mjs web/test/e2e-layout.test.mjs`는 source의 직접 실행 여덟 조합과 재개 여덟 조합이 정확히 한 번 선택되는지, 단일 worker·좁은 grep을 확인한다.
- 첫 전체 검증에서 직접 실행 휴대폰 네 조합도 합계 120초에서 실패해 같은 방식으로 분리했다. 원본 실패를 보존하고 수정 뒤 전체 정본으로 다시 검증한다.
- 직접 실행·재개 실패는 기존 `ChatDiagnostics`에 phase·엔진·폭·harness, HTTP 요청/응답과 연결 대상·수명을 남긴다. 단계의 `diagnose`에서도 저장하므로 바깥 타이머가 끝난 경우도 잡는다. 통과 때는 조합별 phase 경과·연결 수·같은 대상 연속 재접속 수만 출력한다. 실패 자료는 기존 artifact 수집 경로에 둔다.
- 참고: [Playwright WebSocket 사건 시험](https://github.com/microsoft/playwright/blob/main/tests/library/web-socket.spec.ts)(Apache-2.0)은 frame·close를 사건으로 관측하며, [ttyd Xterm](https://github.com/tsl0922/ttyd/blob/main/html/src/components/terminal/xterm/index.ts)([MIT](https://github.com/tsl0922/ttyd/blob/main/LICENSE))은 연결 수명과 구독 해제를 구분한다. [WeTTY](https://github.com/butlerx/wetty/blob/main/src/client/wetty.ts)([MIT](https://github.com/butlerx/wetty/blob/main/LICENSE))와 [xterm 5.5 WriteBuffer](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/common/input/WriteBuffer.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE))의 write 완료 뒤 흐름 제어도 확인했다. 외부 코드·의존성을 제품에 추가하지 않는다.
- 검증은 같은 실행 파일의 기존 묶음 계측·CPU 저하 대조, 독립 조합 E2E93와 전체 `nice -n 15 bash scripts/ci-local.sh`, 공개 검사다. Playwright 1.64.1을 thread 내부에 준비한다. 제품·FRONT_API·AGENT_LINK·E2E 번호는 바꾸지 않는다.

## 안전 모드 복원의 웹 listener 인계(t-0890)

- 26회차 E2E68 원본은 정상 host 시작·웹 listen 뒤 세션 1개의 agent 재연결을 기록했다. pane 20초 실패 시 DOM·요청·프로세스 종료 기록이 없어 그 브라우저의 정확한 실패 경로나 agent 종료 주체를 소급 확정하지 않는다. 수정 전 일반 E2E68도 10.3초에 통과했다. 부하만으로 재현했다고 하지 않는다.
- 실제 안전 모드 serving 종료와 정상 host 초기화 사이를 debug Unix 제어 소켓 `AAA_TEST_SAFE_RELOAD_HOLD`로 붙잡으면 수정 전 TCP 접속은 `ECONNREFUSED`다. 안전 모드는 axum의 graceful shutdown이 listener를 닫기 전에 같은 소켓의 사본을 유지하고 정상 host에 대기열째 넘긴다. 같은 주소와 `:0`은 열린 주소를 이어 쓰고, 「다시 읽기」로 주소를 바꾸거나 웹을 끄면 새 설정을 따른다. 외부 API·프런트 상태·감독자 ready 계약은 바꾸지 않는다.
- `cargo test --locked -p aaa-host --lib safe_mode::`: 실제 HTTP 복원 POST 완료와 안전 모드 serving 종료를 확인한 뒤, 아직 정상 Host가 없는 상태에 HTTP 요청을 넣는다. 정상 웹 인계 뒤 동일 요청의 조작 기기 200 응답을 확인한다. 같은 `:0` 주소 유지·다른 포트로 변경·웹 끄기와 기존 가림·CSRF·Reader·백업 계약도 검사한다.
- E2E68은 같은 제어 경계에서 실제 TCP 접속과 브라우저의 자동 Refresh 요청을 관측한 다음에만 정상 host 초기화를 놓는다. 기존 `pane-t1` 20초 상한·같은 host 프로세스·agent 재연결 단언에 기존 PTY `t1-42` 출력 복원을 더한다. 시간 경과나 준비 표지 완화로 통과시키지 않는다. 제품의 Refresh 3초는 그대로이며 새 시험의 고정 sleep·재시도는 없다.
- 각 단계의 `diagnose`는 화면을 움직이는 공통 진단 전에 `diagnostics/safe-mode/`에 각 기기 DOM·PNG, 요청의 경로·op·상태·실패, navigation·WebSocket, host·agent 로그와 프로세스 PID·exit code·signal·killed를 남긴다. cookie·짝짓기 code·입력 본문은 요청 기록에 넣지 않는다. 종료 사건마다 기록을 갱신해 시험 실패 당시 살아 있던 agent와 harness가 뒤에 죽인 agent를 구별한다. 구판 제어 대조에서는 실패 시 agent가 살아 있었고, harness의 SIGKILL 종료 사건과 같은 시각에 host의 agent 연결 끊김이 생겼다. 이 대조를 원본 종료 주체의 직접 증거로 보지는 않는다.
- 참고는 [NATS Go 재연결 시험](https://github.com/nats-io/nats.go/blob/main/test/reconnect_test.go)의 서버 중지·재연결 callback 관측이다. [Apache-2.0](https://github.com/nats-io/nats.go/blob/main/LICENSE)을 확인했다. 실제 재시작과 접속 완료를 구분하는 방법만 참고하며 코드·의존성을 복사하지 않는다. listener 소유권·graceful 종료는 이미 쓰는 axum 0.8의 `serve::Listener`와 Tokio `TcpListener::into_std`·표준 `TcpListener::try_clone`을 쓴다.
- 최종 검증은 Playwright 1.64.1로 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. GitHub CI를 위한 직접 push는 하지 않는다.

## 앱 색 복원·테마 갤러리(t-0899)

- E2E14는 복원된 앱 GitHub Dark의 `#0d1117`과 ANSI 전체 4.5:1, 기존 GitHub 밝기 짝·공유 저장·재열기를 유지한다.
- E2E95는 Chromium·WebKit 1280·390px에서 adf056a9 앱 색 fixture를 실제 CSS에 덮기 전후 모든 token의 렌더 RGBA를 비교한다.
  Vite의 hex 축약·alpha hex 변환은 canvas 8bit RGBA로 정규화한다. 독립 터미널 bg `#24292e`·원 ANSI와 저장 ID·표시 이름은 유지하고,
  앱을 따르는 터미널은 원 bg·ANSI·커서로 분리한다. PWA·오류색·대화·질문 카드와 기준 화면도 캡처한다.
- E2E96은 같은 4개 엔진/폭 조합에서 새 12종의 실제 선택·host 저장·기기 재열기·OS 밝기 짝·기존 CSS 팔레트 복귀를 검사한다.
  UI 글자·보조·상태·강조·구문색은 기본·판·팝업·hover·입력 바탕과 4.5:1, 주 단추 글자도 4.5:1이어야 한다.
  원 터미널 ANSI는 원색을 보존하며 UI 대비와 구분한다. 팔레트 변경의 colors frame과 재연결 URL의 fg/bg를 각각 검사한다.
  초기 연결은 색을 URL로 보내므로 colors frame을 기다리지 않는다. 설정은 실제 닫힘·history 이동 뒤 다시 읽는다.
- 갤러리의 검색·악센트 무시·빈 결과·선택, 견본 글자·코드의 실제 대비, 미리보기/이름의 카드 안 배치와 가로 넘침을 검사하고 갤러리·테마 화면을 남긴다.
  순수 owner는 기존 GitHub 밝기 짝, 새 원색 자료·ANSI·밝기 짝·설정 보존을 검사한다. 터미널 owner의 독립 GitHub Dark 검사는 유지한다.
- 출처·역할별 이식·개별 라이선스와 보정은 [테마 목록](theme-catalog.md)에 기록한다. 원본 실행 코드나 새 의존성은 추가하지 않는다.
  사건·DOM·HTTP·WebSocket으로 끝내며 고정 대기·재시도·시한 연장은 없다. 정본은 전체 ci-local과 공개 검사다.

## 대화의 실제 1px 이동과 높이 측정(t-0886)

- 원본 t-0883은 Chromium 390px·Codex에서 reset 직후 0, 두 frame 뒤 1px였다. JS 쓰기는 reset 한 번뿐이고 안내 높이는 같았다. 원본에는 브라우저 trace가 없어 당시의 모든 내부 순서를 소급 확정하지 않는다.
- 실제 host·agent·PTY와 원래 360개 메시지를 쓰는 별도 경계 대조에서 같은 0→1px를 재현했다. native 466→467px를 `> 1` 판정이 자동 이동으로 분류해 앵커를 갱신하지 않았고, 가상 row 측정이 466px로 되돌렸다. 이어 scrollend → reset 0 → compositor 1px가 왔다. Chromium trace의 `ScrollableArea::scrollOffsetChanged`는 되돌림·reset을 `kProgrammatic`, 뒤의 1px를 `kCompositor`로 기록했다. 정수·분수 터치 이동량과 reset 실행 경계를 정해 조사했으며 통과한 반복으로 실패를 덮지 않는다. 뒤 표본의 앞 reset에서 온 scrollend가 섞인 경계는 원인 판정에서 제외한다.
- 제품은 자체 쓰기 직후 되읽어 저장한 `lastScrollTop`과 실제 DOM 위치를 정확히 비교한다. 목표의 서브픽셀 값과 되읽은 정수 사이 오차를 실제 사용자 1px 이동과 혼동하지 않는다. scroll 사건·ResizeObserver·row 측정·새 기록 commit의 미반영 이동 보호에 같은 규칙을 적용한다. 복귀 목표의 반올림 허용·끝 판정 문턱은 유지한다.
- E2E93의 새 회귀는 `scrollBy(0, 1)`이 낸 trusted native scroll → 안내 높이 정확히 +1px → ResizeObserver 순서다. 손가락의 물리 입력을 이 API 호출로 대신 검증했다는 뜻은 아니다. 처음 위치는 정확히 1px, 보정 뒤 2px이며 첫 row의 화면 좌표는 같아야 한다. 구판은 top=0·첫 row +2px로 실패했다. 기존 맨 위 안내 높이 변경의 정확한 0, 실제 Chromium touch·scrollend 뒤 reset의 정확한 0, append·prepend·복귀·파일 전환·재개·agent 재시작과 양 엔진·양 폭·양 harness를 유지한다. 시한·기대값 완화·재시도·고정 sleep은 없다.
- 참고 후보는 [TanStack Virtual core](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts)의 실제 scroll offset 관측과 목표/되읽기 구분, [react-window useScrollToIndex](https://github.com/bvaughn/react-window/blob/master/lib/core/useScrollToIndex.ts)의 실측 후 복귀 보정·입력 취소다. [TanStack MIT](https://github.com/TanStack/virtual/blob/main/LICENSE)·[react-window MIT](https://github.com/bvaughn/react-window/blob/master/LICENSE.md)를 확인했다. [Chromium ScrollableArea](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/scroll/scrollable_area.cc)의 programmatic/compositor 구분도 대조했다. 외부 코드나 의존성을 추가하지 않는다.
- 새 회귀의 2→0 복귀도 자기 `scrollend`까지 기다린 뒤 기존 터치를 시작한다. 처음 추가한 검사에서 이 사건을 남기면 다음 터치 종료 대기가 미리 풀려 top=29px가 남는 준비 경합을 확인했다. 종료 사건을 기다리지 않고 두 frame만 더 자거나 시한을 늘려 넘기지 않는다.
- 정본 검증은 Playwright 1.64.1과 격리 브라우저의 전체 `nice -n 15 bash scripts/ci-local.sh --tee`·공개 검사다. 실험용 반복·전역 prototype 계측·CDP trace는 최종 시험에 넣지 않는다.
## 새 자동화의 초기 검사와 이름 입력(t-0882)

- E2E82 원본은 자동화②의 이름만 빈 채 저장 단추가 disabled였다. 원본에는 입력·DOM 교체 trace가 없어 당시 callback 순서까지 소급 확정하지 않는다. 별도 실제 host·브라우저 대조에서 초기 `automation_validate` 전송 직전 이름 칸에 focus·select를 두고 응답을 보류했다. `load`의 loading 반영 뒤 원래 input은 detached, focus는 BODY였으며 브라우저 `insertText("nudge")` 뒤 검사 응답을 놓아도 이름은 빈 문자열이었다. 제목·본문의 외부 동기화가 이름 상태를 덮는 경로는 없다.
- 새 자동화도 loading 상태에서 시작한다. 첫 검사가 끝나기 전 잠깐 노출됐던 입력칸을 제거하는 순서가 사라지며, 이후의 검사·폼/TOML 전환·저장은 기존 경로를 쓴다.
- E2E55의 「초기 검사」 단계는 Chromium·WebKit 각각 1280px·390px에서 실제 초기 응답을 보류한다. fetch 호출 경계의 DOM을 관측해 첫 검사 전 이름 칸 노출을 잡는다(구판 RED). 보류 중 로딩·저장 비활성, 응답 뒤 이름 입력, 같은 input 객체 유지, 제목·본문의 실제 저장을 검사한다. 각 화면에서 다른 화면에 다녀온 뒤 두 번째 자동화도 만들고 저장한다. service worker는 이 대조 화면에서만 막고 응답 내용·effect·입력값은 조작하지 않는다.
- 참고는 [React 제어 입력](https://react.dev/reference/react-dom/components/input)의 동기 상태 반영과 [React 상태 보존](https://react.dev/learn/preserving-and-resetting-state), [React Hook Form createFormControl](https://github.com/react-hook-form/react-hook-form/blob/master/src/logic/createFormControl.ts)의 초기 isLoading·외부 값 동기화다. [React Hook Form MIT](https://github.com/react-hook-form/react-hook-form/blob/master/LICENSE)를 확인했다. 외부 코드·의존성은 추가하지 않는다.
- 수정 전 결정적 대조와 회귀 RED를 보존하고 수정 뒤 E2E55·82, 전체 `nice -n 15 bash scripts/ci-local.sh --tee`·공개 검사를 실행한다. 기존 20초 사건 대기·30초 locator·120초 단계 상한을 유지하며 고정 sleep·재시도·기대값 완화는 없다.

## Rust 질문 재알림과 큰 실행 파일 처리(t-0864)

- 원본 m13 DB에는 마지막 `make test` 권한 질문에 답한 뒤 831ms에 같은 본문·같은 사건 시각의 질문이 다시 열리고, 이어 idle에서 Gone이 된 기록이 있다. agent 원문에도 입력 → question 재감지 → idle 순서가 있다. 가짜 Claude의 Esc 처리는 바이트마다 `stty icanon`을 복원하고 suffix 구별용 `stty`·`dd`를 거치는 동안 답한 창을 남겼다. 제품은 입력 뒤 800ms에도 남은 창을 다시 묻는 기존 계약을 유지한다.
- `claude_menu_dismisses_escape_before_waiting_for_its_suffix`는 실제 PTY의 suffix 읽기 직전에 표지를 넣고 그 이전 출력만 Vt로 해석한다. 구판은 권한 질문이 그대로 있어 결정적으로 실패한다. fixture는 Esc를 읽으면 창을 먼저 지우고 화살표이면 다시 그린다. 키마다 canonical 모드를 복원하지 않고 입력 상자를 열 때 복원한다. 기존 owner의 질문 5개·답·흐름·Enter 미전송·취소·화살표 선택·interrupt 단언과 30초는 그대로다.
- 24회차 후속: 창을 지운 뒤 빈 화면을 남기면 suffix의 `stty`/`dd`가 끝나기 전에 화면 감지가 `Idle(rule=None)`을 알렸다. 이때 보낸 `long run`의 첫 `l`을 suffix가 소비하면 다음 줄은 `got: ong run`이 되고, 메시지 accepted·질문 5개가 모두 맞아도 interrupt의 앞선 받음 단언은 실패한다. 실제 PTY에서 첫 Esc와 다음 입력 시점을 고정해 바이트 소비를 확인했다. 창 지우기·canonical 복원 변경을 각각 대조하면 조기 Idle은 창 지우기 변경에서만 생긴다. 답한 창은 즉시 지우면서 같은 출력에 작업 중 표지를 남기고, suffix 구별 뒤 재그리기/최종 지우기에서 그 표지도 지운다. 위 회귀는 질문 부재와 함께 실제 감지기의 `Working`·`visible_working`도 검사하며, owner의 실패 단언에는 interrupt 반환 화면 전체를 싣는다. 기다림을 추가하거나 기존 질문·interrupt 순서·횟수·시한을 바꾸지 않는다.
- 원본 update 실험실에는 203,079,104바이트 staging과 126,328,832바이트의 설치 임시 파일이 남았다. 실패 지점은 PTY EOF가 아니라 macOS의 새 inode로 하는 `std::io::copy` 도중이다. APFS는 `fclonefileat`으로 별도 inode를 만들고, clone 미지원/다른 볼륨만 `create_new`로 확보한 FD에 `fcopyfile(COPYFILE_DATA)`한다. 기존 경로는 덮지 않고, 전체 SHA-256 재검증·실행 권한·보관·rename은 유지한다. 다른 오류를 다시 시도하지 않는다.
- `fresh_install_image_has_its_own_inode_and_never_overwrites_an_existing_destination`는 clone과 FD 복사 양쪽의 정확한 bytes·별도 inode·기존 목적지 거부·정리를 검사한다. 원본 실행 파일과 clone의 변경도 서로 전파되지 않는다. 실제 update owner는 원래 교체·보관·두 번 rollback·keep·self-check 전체와 20초를 유지한다.
- install 원본에는 yes 뒤 EOF 상한과 빈 출력 상한만 있어 당시 stack은 없다. 별도 실제 승인 CLI에서 yes 뒤 `sample`은 `review::confirm`의 계획 재검사와 preview 아래 SHA-256 계산을 가리켰다. 해당 경로는 큰 debug 실행 파일을 반복해서 읽는다. 감지·payload·snapshot의 SHA-256을 기존 ring 구현으로 계산하며 검사를 생략하거나 cache하지 않는다. 이 stack은 원본 세 실행의 순간을 소급 증명하지 않는다. 자식 종료·PTY reader·기존 60초는 바꾸지 않는다.
- 두 digest 시험은 빈 입력·독립 sha2 기준·64KiB 양쪽 경계·끝 조각·같은 경로/길이의 새 bytes, 디렉터리·1GiB 초과 거부를 검사한다. snapshot의 symlink 거부·권한·inode·수정 시각 검사는 유지한다. 정본 검증은 기존 `cargo test -p aaa --test install`, `--test e2e`, `--test m13`, `--test deps`를 포함하는 전체 `scripts/ci-local.sh`다. 기존 launchd CI 전용 ignored owner는 변경하지 않는다.
- 참고: [Pexpect READY 시험](https://github.com/pexpect/pexpect/blob/master/tests/test_winsize.py)([ISC](https://github.com/pexpect/pexpect/blob/master/LICENSE)), [portable-pty 출력·종료 예제](https://github.com/wezterm/wezterm/blob/main/pty/examples/bash.rs)([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)), [expectrl Session](https://github.com/zhiburt/expectrl/blob/main/src/session/mod.rs)([MIT](https://github.com/zhiburt/expectrl/blob/main/LICENSE))를 확인했다. 준비 뒤 입력하고 출력과 종료를 구분하는 방식을 따르며 고정 입력 지연은 적용하지 않았다. macOS 복사는 [Rust std의 clone/FD 복사 경로](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs)(MIT/Apache-2.0)와 [Apple copyfile](https://github.com/apple-oss-distributions/copyfile/blob/main/copyfile.c)(APSL-2.0)의 API를 확인했다. 외부 소스 코드는 복사하거나 실행하지 않았고 의존성도 추가하지 않았다.

## m13 재시작 뒤 보고 수집 완료 사건(t-0880)

- main `b6d7bda8`의 [CI 원문](https://github.com/cLazyZombie/aaa/actions/runs/39915021368)은 Ubuntu·macOS 모두 실제 사건 `[]`, 기대 `["report_ready"]`다. 추가 사건이나 harness 재전송 중복이 아니다. `orch.rs`의 수집은 `next`·`report_hash`를 먼저 저장하고 질문 정리 뒤 `report_ready`를 기록하므로, 상태 조회만으로 사건 발행 완료를 판정할 수 없다.
- `remote_dir_thread_survives_host_restart`는 새 host의 `lab.follow`로 해당 thread·기준 seq 뒤 `report_ready`를 받은 뒤 상태와 정확한 사건 목록을 검사한다. 기존 follow의 원장 재생을 써 구독 전에 발행된 사건도 놓치지 않는다. 새 보고 본문·검토 묶음·정확히 사건 하나·outage 내구성 단언과 기존 30초 상한은 유지한다. 고정 대기·시한 증가·재시도·ignore·제품 변경은 없다.
- `report_events::report_state_precedes_completion_event`는 기존 `AAA_TEST_FLOW_HOLD`로 발행 순서 잠금을 잡고 실제 수집의 새 `next` 저장과 `report_ready`의 잠금 대기 표지를 확인한다. 그때 사건 목록은 비어 있다. 구판 owner의 단언을 같은 지점에 놓은 대조는 CI와 같은 `[] != ["report_ready"]`로 실패했다. 해제 뒤 live 사건과 정확히 한 건, 새 구독의 원장 재생을 모두 검사한다. 제어 소켓 외 제품 경로를 대체하지 않는다.
- 참고 후보는 [NATS JetStream 중복 판정과 ACK](https://github.com/nats-io/nats.docs/blob/master/using-nats/jetstream/model_deep_dive.md)([server Apache-2.0](https://github.com/nats-io/nats-server/blob/main/LICENSE))와 [Temporal의 사건 이력 재생](https://github.com/temporalio/sdk-rust/blob/main/arch_docs/sdks_intro.md)([MIT](https://github.com/temporalio/sdk-rust/blob/main/LICENSE.txt))다. 원장에 기록된 완료와 상태 관측을 구분하고 기존 순번으로 재생하는 방식을 따른다. 중복 원인이 아니므로 새 멱등 키·의존성·외부 코드는 추가하지 않는다.
- 검증은 두 owner의 정확한 이름 실행, `nice -n 15 bash scripts/ci-local.sh` 전체와 공개 검사다. 새 회귀는 Unix m13 전용이며 t-0864의 Claude fixture·t-0874의 interrupt 실패 영역을 바꾸지 않는다.

## E2E03 연속 페이지 이동의 늦은 native scroll(t-0870)

- 원본 실패는 30행 화면의 `s-143`·native top 2698px/끝 3249px였다. E2E03 일반 실행은 수정 전에도 통과했다. PageUp의 DOM 동기화 → PageDown → 앞선 native scroll 순서를 고정하면 구판이 같은 위치에서 기존 20초 끝 이동 단언에 실패한다. 재연결·resize는 이 순서를 노출하는 배경이며 오래된 `top0` 기대값이나 키 누락이 직접 원인은 아니다. 원본에는 브라우저 내부 호출 trace가 없어 당시 모든 callback 순서까지 소급 확정하지 않는다.
- xterm 패치의 자체 동기화 판정은 현재 `buffer.ydisp` 대신 `_innerRefresh`가 DOM에 쓴 뒤 되읽은 위치를 쓴다. 자체 사건을 무시한 뒤 `syncScrollArea()`도 호출한다. 다음 키가 사건 전의 옛 `_lastScrollTop`을 보고 DOM 동기화를 생략할 수 있기 때문이다. 사용자가 다른 위치로 굴린 사건과 reset 중 0 높이 보호는 유지한다.
- `node --test web/test/xterm-source.test.mjs`는 실제 npm upstream 메서드에 배포 patch를 적용해 실행한다. PageUp 뒤 논리 목표만 바뀐 경계, 늦은 native 사건, 다음 frame의 실제 DOM 위치, 19px·18.5px의 반올림, 자체 동기화와 합쳐진 사용자 이동을 검사한다. 구판은 이동량 -29/기대 0으로 실패했다. 논리 위치만 고친 중간판은 브라우저에서 DOM 2698px/기대 3249px로 실패했다.
- E2E03의 새 단계는 실제 host·agent·PTY를 두고 Chromium·WebKit 각각 새 연결의 snapshot ACK → 연결 종료 → 새 연결의 snapshot ACK → viewport 변경 → 새 PTY grid와 실제 렌더 순서로 준비한다. PageUp이 만든 trusted native scroll의 capture 경계에서 같은 앱 PageDown 단축키를 호출한다. 버퍼·scrollTop·frame 값을 조작하지 않고 사건 순서만 고정하며, 최종 첫 줄·렌더의 top/base·native scroll 끝을 모두 확인한다. 뒤의 맨 위 이동은 native overflow 경로이며 실제 손가락이나 휠 조작을 재현했다는 뜻은 아니다. 기존 실제 키의 한 화면 이동·키 누출·alternate screen 단언도 유지한다.
- 참고는 [xterm.js 5.5 Viewport](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)의 비동기 DOM 동기화와 [CoreTerminal](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/common/CoreTerminal.ts)의 `scrollPages`·`scrollToBottom`, [VS Code XtermTerminal](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts)의 페이지/끝 이동 위임이다. [xterm MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)·[VS Code MIT](https://github.com/microsoft/vscode/blob/main/LICENSE.txt)를 확인했다. xterm의 기존 API와 frame/scroll 분리를 따르며 외부 코드·의존성을 추가하지 않는다.
- 정본 검증은 Playwright 1.64.1·격리 브라우저로 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다. 실패 재시도·고정 sleep·시한 증가·기대값 완화는 없다.

## Windows 설치기 aaa setup(t-0878)

- 흐름은 `cargo test -p aaa --bin aaa setup::`가 macOS에서 본다. 가짜 `System`(SCM·LSA·PATH·등록)과 정해 둔 답의 `Ui`를 쓴다. 질문 순서, 바꾸기 전 점검, 바꾸는 순서(등록 → PATH → 권한 → 서비스 로그온 → 등록 → 시작), 실패·취소의 되돌림(옛 실행 파일·설정 원문·PATH 원문·새 빈 폴더·승인 전 장치 키), 등록 유지 뒤 재실행, 업데이트, 제거와 `--purge`, 남의 서비스 보존, 같은 계정의 권한 보존, 계획 칸 맞춤을 단언한다. 시간은 쓰지 않는다(`pause`가 즉시 돌아온다).
- Windows 코드는 `cargo xwin clippy -p aaa --bin aaa --target x86_64-pc-windows-msvc -- -D warnings`로 검사한다. 실기는 Windows 시험 PC 규칙(memory machines)대로 전용 폴더·`aaa-test-0878-*` 이름·가짜 관리자 사용자·기준 목록 대조로 한다. 시험 host는 다른 머신에 별도 상태 폴더·포트로 띄우고 운영 host에는 붙지 않는다. 대화형 SSH는 pty 드라이버가 출력 사건(질문·확인 코드·완료 줄)을 기다려 답한다. 승인은 그 시험 host CLI(`AAA_SESSION_ID` 없는 환경)로 한다.
- 실기 확인 항목: 등록 창 닫힘→Enter, 틀린 암호→다시, 승인, 서비스(계정·세션 0·지연 자동 시작·실패 동작), host 연결, 서비스 pane의 사용자 PATH(표지 `codex.cmd`), 1바이트 다른 실행 파일로 업데이트, 이름 충돌→새 이름, 승인 대기·질문 중 Ctrl+C(130과 되돌림), 터미널 없는 SSH(2), 관리자가 아닌 SSH(2), 제거·purge 뒤 권한 회수. 탐색기 더블클릭(UAC 창)은 사용자 데스크톱에 창을 띄우므로 사용자 확인으로 남긴다.
- 정리 주의: 가짜 사용자로 비밀번호 SSH 로그온·서비스 로그온을 한 뒤에는 그 사용자를 지워도 프로필 hive(`HKU\<SID>`·`_Classes`)가 `Loaded`로 남을 수 있다. 그 SID의 프로세스·로그온 세션이 없어도 남았고 `reg unload`는 거부됐다. 재부팅·sshd 재시작은 하지 않으므로 그 프로필 폴더는 다음 재부팅 뒤 `Remove-CimInstance`로 지우고 보고에 남긴다. BITS 시작 방식은 Windows가 Auto↔Manual로 바꾸므로 기준 대조에서 시험 변경으로 세지 않는다(t-0853과 같은 관찰).
## E2E97 회선 제한기의 FIFO 보존(t-0919)

- 기존 main의 로컬 재현은 ANSI·1 Mbit/s·100ms·identity에서 마지막 ACK가 snapshot 395,283 B + live 348 B = 395,631 B까지 왔는데 echo 탐색이 끝나지 않았다. 추가 binary 계측에서는 한글 표본의 같은 조건에서 `abcdefgh`가 `bcdeafgh`로 PTY에 도착했다. 압축 해제나 xterm flush 누락으로 판정하지 않는다.
- 원인은 시험 회선의 메시지별 독립 타이머다. 절대 도착 시각이 증가해도 상대 지연을 다시 계산해 소수 밀리초를 버리는 Node 타이머는 등록 지연에 따라 메시지를 앞지를 수 있다. 방향마다 `RateLimitedWire`의 FIFO와 타이머 하나를 두며, 기존 byte-rate·RTT 식으로 계산한 도착 시각을 확인한 뒤 전달한다. 양방향은 독립이며 연결 종료는 큐·타이머를 폐기한다.
- `node --test web/test/rateLimitedWire.test.mjs`는 실제 쓰는 회선 owner에 Node의 소수 지연 절삭과 등록 중 시계 진행을 넣는다. 구 독립 타이머는 `bacdefgh`로 실패하며, 수리는 `abcdefgh`를 전달한다. 큰 history 뒤 binary·state 순서, 예정 시각 이전 미전달, 반대 방향 입력의 독립, 종료 뒤 자료 폐기도 확인한다. 실제 시간을 자거나 실패를 재시도하지 않는다.
- E2E97은 기존 Chromium/WebKit × 1280/390px와 일반·ANSI·한글 × identity/gzip × 세 회선 조건을 모두 유지한다. 8키의 바이트 수에 실제 순서 단언을 더하고, PTY 대기와 최종 xterm ACK 대기를 함께 구독하여 거절 시 진단·정리가 빠지지 않게 한다. 실패 로그에는 송신 순서·binary 끝부분·바이트 수·stream별 ACK를 남긴다. 기존 ACK 20초·단계 120초·기대 echo와 history prepend 조건은 그대로다.
- 참고: [xterm.js flow control](https://xtermjs.org/docs/guides/flowcontrol/)의 write 완료 callback과 [ttyd writeData](https://github.com/tsl0922/ttyd/blob/main/html/src/components/terminal/xterm/index.ts)의 흐름 제어를 조사했다. [xterm.js MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)·[ttyd MIT](https://github.com/tsl0922/ttyd/blob/main/LICENSE)를 확인했다. [Node 타이머 계약](https://nodejs.org/api/timers.html#settimeoutcallback-delay-args)의 소수 지연 절삭·실행 순서 비보장을 대조했다. 외부 코드·의존성과 제품 API 변경은 없다.
- 정본은 기존 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. GitHub CI는 기다리지 않는다.

## 터미널 기록 압축(t-0876)

- `cargo test -p aaa-common terminal_payload --locked`: 두 codec의 정확한 일반·색상·한글·binary 왕복, 4095/4096 B와 이득 경계, 768 KiB/원시 JSON frame 상한, gzip CRC·잘림·남은 데이터·압축 폭탄, zstd window/dictionary, null·알 수 없는 metadata, IPC 거부를 검사한다. 실제 frame writer의 snapshot→binary→Pong 순서와 원시 offset, blocking 작업 두 개와 취소된 작업의 permit 보유도 사건으로 확인한다.
- `cargo test -p aaa-agent -p aaa-host terminal_compression_tests --locked`: 실제 Hello/Welcome 처리에서 원격/로컬·선택 기능 유무·identity/압축의 조합과 재연결 초기화를 검사한다. 허용하지 않은 압축은 소비 전에 링크 오류가 나야 한다. 구판 필드 부재와 생성 TypeScript도 기존 proto 시험으로 확인한다.
- `node --test web/test/terminalCompression.test.mjs`: 실제 native gzip의 CRC·길이·폭탄·취소와 Worker gate 대역을 사용한다. 해제 전 소비 없음, snapshot/binary/history/새 stream 순서, 늦은 결과 폐기, 구 host identity, 지원 실패, 재연결의 새 Worker 준비, raw_len 위조를 포함한 wire/raw 대기 예산을 검사한다.
- E2E97 `97-terminal-compression`은 실제 PTY·host·agent·제품 Worker·xterm으로 Chromium/WebKit의 1280/390px에서 한글·색·줄 접힘·원시 길이 ACK·입력·재연결·두 쪽 prepend를 검사한다. 기기 크기를 먼저 적용한 뒤 기록을 생성하여 resize로 무효가 된 anchor를 압축 결함과 혼동하지 않는다. Worker 첫 해제를 gate로 잡아 ACK/표시가 앞서지 않는지 확인한다. 기존 E2E07의 스크롤·reset·즉시 wake 재연결·4008·host 재시작/session-host fallback owner도 유지한다.
- E2E97의 회선 대조는 같은 source의 identity/gzip을 실제 WS 경계에서 byte-rate로 제한한다. 일반·색상·한글 표본을 1 Gbit/s/1ms·5 Mbit/s/50ms·1 Mbit/s/100ms 조건에서 비교하고 실제 wire/원시 길이·마지막 xterm ACK까지의 시간과 history 중 8키의 정확한 송신·PTY 전달·xterm 적용 ACK를 기록한다. 읽던 history를 유지하는 화면에서 키 echo가 viewport에 보인다고 가정하지 않는다. NIC/시스템 설정·기존 실패 시한은 바꾸지 않으며 고정 sleep·실패 재시도는 없다.
- 필수 정본은 `nice -n 15 bash scripts/ci-local.sh --tee`, 의존 경계 시험·Linux/Windows strict cross·공개 검사다. 구판 실제 binary 조합, 제한 회선의 원격 frame, native Worker 메모리·취소, Windows native zstd C 빌드와 Edge/Safari 실기는 별도 실측 산출물로 구분한다. WebKit headless를 실제 Safari로, Edge headless를 설치된 PWA로 쓰지 않는다.


## 보고서 교체 시험의 Windows 파일 닫힘(t-0862)

- `node --test web/test/reportFile.test.mjs`는 Windows에서 원래 writer의 남은 출력을 쓰고 FD를 닫은 뒤 보고서를 교체한다. POSIX에서는 기존처럼 교체 뒤 옛 FD에 늦게 쓰는 대조를 유지한다. 두 경로 모두 최종 본문 전체가 정확히 일치해야 하며 Windows 시험도 건너뛰지 않는다.
- 별도 공통 시험은 실제 `renameSync` 직전에 원래 보고서가 그대로 있고 같은 폴더의 임시 보고서가 전체 본문을 담았는지 검사한다. 실제 rename 한 번 뒤 최종 본문과 임시 파일 제거도 확인한다. 직접 덮어쓰기로 되돌리면 이 계약에서 실패한다.
- E2E87은 파일 존재 대신 가짜 agent의 `asked-cards` 출력 사건을 기다린다. 이 출력은 `printf > report.md`가 끝나 FD가 닫힌 뒤에만 나온다. 파일의 마지막 글자를 읽는 것만으로는 close가 보장되지 않는다. 기존 20초 출력 대기와 8개 화면 조합·스크롤 단언은 유지하며 고정 sleep·교체 재시도는 추가하지 않는다. 웹 E2E 전체 harness는 Unix 전용이고 Windows에서는 같은 공개 helper를 쓰는 Node 시험을 실제 실행한다.
- 참고 후보: [write-file-atomic의 close 뒤 rename](https://github.com/npm/write-file-atomic/blob/main/lib/index.js)([ISC](https://github.com/npm/write-file-atomic/blob/main/LICENSE.md)), [python-atomicwrites의 close 뒤 commit과 Windows 교체](https://github.com/untitaker/python-atomicwrites/blob/master/atomicwrites/__init__.py)([MIT](https://github.com/untitaker/python-atomicwrites/blob/master/LICENSE)). 완성·close·공개 순서만 따르고 외부 코드나 의존성은 가져오지 않는다. [Node fs.renameSync](https://nodejs.org/api/fs.html#fsrenamesyncoldpath-newpath)는 rename을 설명하며, [libuv Windows 구현](https://github.com/libuv/libuv/blob/v1.x/src/win/fs.c)의 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` 경로도 확인한다. 열린 대상 교체의 EPERM은 Windows 실기 대조로 확인한다.
- 정본 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`·공개 검사다. 착지 전 Windows 실기에서 수정 전 실패·수정 후 두 시험 통과와 실험실 정리·기준 목록 대조를 별도로 기록한다.

## 알림 중 터미널·대화 입력(t-0859)

- E2E94 `94-notice-input`은 실제 host·agent·PTY를 띄우고 알림만 events WebSocket 경계에 주입한다. Chromium 3840px(DPR 2)·390px, WebKit 1280px·390px에서 각기 새 pane을 만든다. 알림 세 번의 등장·교체와 보고서 배지·질문/알림함 갱신 동안 6 frame마다 키를 보내며(60Hz에서 약 10타/초), 애니메이션 완료 사건으로 끝낸다. 고정 sleep·시한 연장은 없다.
- 터미널은 보낸 글의 정확한 순서와 실제 PTY에서 돌아온 전체 글을 검사한다. 대화는 초안 전체가 같은지 검사하며 두 보기 모두 focusin 사건 없음·동일 textarea 유지·키 누락 없음·`role=status`/`aria-live=polite`를 본다. 필터 상자의 폭이 실제 카드보다 크면 실패한다. 구판은 첫 4K 알림에서 필터 3840px·카드 396px로 실패했다.
- 키→DOM 반영 뒤 첫 frame(터미널은 PTY 왕복 포함)의 중앙값·95백분위·최대와 frame 간격·50ms 이상 작업을 진단으로 출력한다. 공유 머신의 속도 수치를 통과 조건으로 삼지 않는다. 회귀 판정은 필터 처리 범위와 정확한 입력·포커스다. Windows Edge 실사용과 headless macOS 계측을 동일시하지 않는다.
- 참고 후보는 [VS Code 알림](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/notifications/notificationsToasts.ts), [react-hot-toast Toaster](https://github.com/timolins/react-hot-toast/blob/main/src/components/toaster.tsx), [Sonner Toaster](https://github.com/emilkowalski/sonner/blob/main/src/index.tsx)다. 세 프로젝트의 MIT 라이선스를 확인했다. 알림의 제한된 영역·포커스 유지·polite 알림 방식을 참고했고 코드나 의존성은 가져오지 않았다.
- 전체 정본은 Playwright 1.64.1/격리 브라우저로 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다. 기존 E2E12·19의 알림 클릭·닫기·휴대폰 배치·소리 경로도 유지한다.

## Windows SSH 적용부터 SCM·TLS 등록까지(t-0857)

- 적용 stream의 종료 코드·stdout/stderr·결과는 `cfg(test)`의 실험실 로그에만 남긴다. 공개 DTO·DB에 원문을 추가하지 않는다. 암호의 원문과 JSON escape 표기를 로그 직렬화 전에 가린다. Windows owner의 대기·잘못된 암호·최종 적용 단언에는 error와 install 결과를 함께 남겨 다음 단계의 실패도 구별한다.
- 실제 Windows SSH에서 첫 적용은 `password_stdin_source_required`/2였다. `--resume --secrets-stdin`은 저장된 서명 계획의 입력 정책을 쓰므로 CLI 선택이 비어 있을 때만 재개를 허용한다. 최초 적용의 stdin 선택 요구·명시적 정책 변경 거부·stdin 충돌·세션 거부·서명/nonce/EOF 검증은 유지한다. `aaa-config`의 `protected_resume_uses_the_saved_plan_without_changing_password_choices`는 구판 검증에서 실패한다. 실제 CLI의 `authorization::protected_stdin_resumes_the_signed_plan_and_rejects_an_unused_password_before_writes`는 미사용 암호의 쓰기 전 거부·자료 불변 뒤 같은 ID/계획으로 실제 파일 worker까지 실행한다.
- 다음 실제 실패는 SCM 등록 뒤 계정 관찰의 `service_policy_unverified`였다. SCM의 로컬 약칭 `.\user`를 `LookupAccountNameW`에 그대로 주면 ERROR_NONE_MAPPED/1332가 난다. OS의 `GetComputerNameExW(ComputerNameNetBIOS)`로 정규화하고 기존 SID·계정 종류·설정 일치 검사를 유지한다. 환경 변수의 머신 이름은 쓰지 않는다. Windows 유닛 `scm_dot_account_and_os_qualified_account_resolve_to_the_same_sid`는 실제 계정 이름/SID 왕복과 로컬 약칭의 같은 조회 결과를 검사한다.
- 별도 instance의 긴 Windows 경로는 서비스 backend 시작 전에 Unix 소켓 길이 검사로 거부됐다. agent의 이 검사는 Unix에만 적용한다. Windows의 Named Pipe 이름·ACL·준비 사건은 기존 IPC 구현을 그대로 쓴다. 실기 owner는 긴 경로의 별도 instance에서 암호 없는 대기→가짜 암호 거부→같은 ID 재개→SCM agent→실제 TLS 연결까지 기존 단언과 시한으로 검사한다. 시험 빌드의 thread-local instance 선택만 격리용으로 추가하며 제품의 기본 머신 추가 선택은 바꾸지 않는다.
- 원래 owner의 마지막 영속 기록 검사에 도달하면 SYSTEM sshd의 `-E` 로그가 share-read를 막아 ERROR_SHARING_VIOLATION/32가 났다. Host runtime을 닫고, 부모가 만든 실험실 sshd의 서비스 중지와 기록한 프로세스의 실제 종료를 확인한 뒤 로그를 포함한 모든 자료를 검사한다. 파일 제외·읽기 오류 무시·재시도는 없다. CI 부모는 소유한 서비스 이름을 시험 자식에 전달한다. 이 순서로 실기 전체 owner가 27.22초에 통과했으며 실제/잘못된 가짜 암호 양쪽의 영속 원문 부재도 확인했다.
- 참고 후보는 [windows-service-rs](https://github.com/mullvad/windows-service-rs/blob/main/src/service.rs)(MIT/Apache-2.0), [PowerShell SSHProcessMediator](https://github.com/PowerShell/PowerShell/blob/99a49e0eab9b137449b690ab4607d7932c6f9956/src/System.Management.Automation/engine/remoting/server/OutOfProcServerMediator.cs)(MIT), [Win32-OpenSSH fd 초기화](https://github.com/PowerShell/openssh-portable/blob/274893f8d1fe86c4d128b514c3af4bc10cdc0c99/contrib/win32/win32compat/w32fd.c)(BSD 계열)다. 기존 SCM·raw stdin 경계를 유지하고 외부 코드를 복사하지 않았다. 계정 조회는 [Microsoft LookupAccountNameW 문서](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-lookupaccountnamew)의 정규 계정명 규칙을 따른다.
- 검증은 실기 전체 owner·Windows 계정 조회 유닛·독립 production release build, 전체 `nice -n 15 bash scripts/ci-local.sh --tee`, Windows/Linux strict clippy와 공개 검사다. 실기에는 정확한 시험용 서비스·사용자·프로필만 만들고 기준 목록과 대조한다. 기존 sshd·기존 설치·실제 암호·재부팅은 쓰지 않으며 GitHub 진단 push는 사용하지 않는다.

## 기기별 터미널 색 테마(t-0853)

- `node --test web/test/terminalTheme.test.mjs`: WezTerm 정본 fixture의 ANSI 16색·커서·선택 색을 그대로 쓰고 사용자 bg/fg만 덮는지 확인한다. 모르는 선택→앱 기본, 독립 테마의 앱 token 무참조, 반환값 변형 격리, CSS 단축 hex→OSC 6자리도 검사한다.
- E2E06의 「터미널 색 테마」 단계는 Chromium 1280·WebKit 390에서 실제 설정 선택, ANSI 16색 원색·여백·글꼴/크기 유지·OSC colors 전송, 앱 팔레트와 독립, 기기 저장·새 기기 미공유·같은 기기 다른 탭·새로고침·기본 복원·선택 상자 넘침을 확인한다. DOM renderer 자체는 투명하고 `.term-scroll`이 바탕을 그리므로 글자 뒤 처음 칠해진 조상의 색까지 대조한다. 초기 시험이 xterm 요소 자체의 배경을 불투명하다고 가정해 실패한 것을 확인하고 이 실제 그리기 경로로 검사를 바로잡았다. 제품 기대 색·시한을 완화하지 않는다.
- E2E20은 기존 0·1·2화면 질의/중복 답 금지 단언을 유지한다. 두 번째 기기에서 독립 테마를 고른 직후·재접속·기본 복원 때 PTY가 받는 OSC 10/11 전체 답을 고정 기대 색과 대조한다. xterm이 별도로 답을 보내지 않는 것도 검사한다. 실제 session host 경로를 쓰며 고정 sleep·재시도는 없다.
- Windows 시안은 실기 Edge의 격리된 프로필과 실제 aaa host/agent/web 빌드를 쓴다. 같은 고정 출력·13px·150%·3840×2160에서 앱 GitHub Dark/WezTerm 사용자 색 각각 기본 LCD/회색조 합성을 대조한다. 사용자 화면 배율·ClearType·설치 글꼴은 바꾸지 않는다. 회색조 합성은 시안에만 넣고 제품 CSS에는 적용하지 않는다. 정본 검증은 전체 ci-local·공개 검사다.

## 요약 카드와 무관한 scroll 분리(t-0863)

- 원본 E2E28 두 실패의 DOM·사건 trace는 이 작업 머신에 없다. 수정 전 Chromium 감속 4배·worker 2의 E2E16·28·84 전체와, 감속 6배·worker 3의 E2E28 고정 6회는 통과했다. 이 관측만으로 원본 실패의 사건 순서를 소급 확정하지 않는다.
- 별도 결정적 대조는 새 화면에서 실제 `thread_summary` 응답을 붙잡고 hover 타이머 뒤 실제 셸 PTY에 100줄을 쓴다. xterm의 native viewport scroll을 확인하고, thread 행의 위치·포인터가 그대로인 상태에서 응답을 놓는다. 구판은 기존 `summary-hover visible` 30초 대기에서 실패한다. scroll capture가 anchor를 지워 `useSummary` effect를 정리하고, 도착한 응답을 버리는 제품 경합이다. 가상화나 마우스 진입 누락이 이 대조의 원인은 아니다.
- `useSummaryHover`는 scroll 대상이 anchor를 포함할 때만 닫는다. 터미널 출력·대화 내부 scroll에는 카드와 응답 대기를 유지하고, 목록을 굴리면 기존처럼 닫는다. 행에 focus가 있는 경우·resize·마우스 이탈·누름·메뉴 억제·150ms/400ms 규칙은 유지한다.
- E2E28의 새 회귀는 Chromium·WebKit 1280px에서 응답 전 scroll, 카드 표시 뒤 출력 scroll, 카드 DOM의 같은 객체 유지, 실제 목록 scroll의 닫힘을 검사한다. 새 대조 화면에만 service worker를 차단하여 route gate 우회를 막는다(E2E37과 같다). 기존 화면·브라우저·폭·테마·모델 전환 단언과 30초 locator·20초 DOM·120초 단계 상한은 유지한다. 고정 sleep·재시도는 추가하지 않는다.
- hover visible 대기에는 단계 이름을 붙인다. `SummaryDiagnostics`는 포인터 진입·이탈·누름, scroll 대상과 행의 포함 관계·자리, 요약 HTTP 요청·응답, 카드 표시 전이를 제한된 메모리에 기록한다. 실패 때만 `diagnostics/summary-*`에 JSON·화면을 저장하고 공통 termDiag보다 먼저 수집한다. 제품 타이머를 바꾸거나 성공 경로에서 파일을 쓰지 않는다.
- 참고 후보: [Radix Tooltip](https://github.com/radix-ui/primitives/blob/main/packages/react/tooltip/src/tooltip.tsx)의 진입·이탈 타이머 취소와 [Floating UI useDismiss](https://github.com/floating-ui/floating-ui/blob/master/packages/react/src/hooks/useDismiss.ts)의 `ancestorScroll` 범위다. [Radix MIT](https://github.com/radix-ui/primitives/blob/main/LICENSE)·[Floating UI MIT](https://github.com/floating-ui/floating-ui/blob/master/LICENSE)를 확인했다. 기준 행을 움직이는 스크롤과 무관한 스크롤을 구분하는 방식을 따르고 외부 코드·의존성은 가져오지 않았다.
- 정본 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`·의존 경계 시험·공개 검사다. Playwright 1.64.1과 브라우저를 자기 작업 폴더에 격리한다. E2E16·84는 재현된 실패가 있을 때만 수정하며, 통과 실행은 원본 간헐 실패의 부재 증명으로 취급하지 않는다.

## Windows SSH 원격 install의 명시적 상태 경로(t-0847)

- Windows 래퍼는 `install-probe`·`install-bootstrap`·`install --resume` 모두에 현지 `LOCALAPPDATA/aaa/state`를 한 인자로 전달한다. LOCALAPPDATA가 비어 있으면 USERPROFILE을 써서 `aaa-config::paths::default_state_dir_for(None)`과 맞춘다. 전역 AAA_STATE_DIR이나 host 머신의 경로를 전달하지 않는다.
- 실기에서 수정 전 제품 래퍼는 exit 2·빈 stdout/stderr였다. CLI가 명시적 상태 경로 누락으로 SSH 검증 전에 끝난다. 수정한 제품 래퍼는 공백이 든 LOCALAPPDATA와 값이 없는 USERPROFILE fallback에서 각각 exit 0·`planned` 제안을 반환했다. 실제 SYSTEM OpenSSH 계보를 검사했으며, 실험실 안 환경만 바꾸고 설치·SCM 등록은 실행하지 않았다.
- `cargo test --locked -p aaa --test install ssh_executors_report_missing_explicit_state_without_writing`은 probe/bootstrap/worker의 필수 경로 누락을 실제 argv로 실행해 exit 2·명확한 stderr·비어 있는 stdout과 파일 불변을 확인한다. `cargo test --locked -p aaa-host --lib machine_setup::`는 세 Windows 명령의 공통 경로 전달, 원격 실패와 잘못된 JSON의 오류 처리·공개 오류에 원문이 없는 것을 검사한다. 기존 Windows CI의 실제 SYSTEM sshd→계획→bootstrap→SCM→TLS 등록 owner는 그대로 유지한다.
- 실패한 probe의 종료 코드·stderr·stdout(JSON 포함)은 `cfg(test)`의 실험실 owner 로그에만 남긴다. 경로·계정이 든 원문을 제품 DTO/DB에 저장하지 않는다. CI 부모는 기존처럼 가짜 암호가 stdout/stderr에 있으면 출력 전에 실패시킨다. SSH 거부 3·충돌 4와 모든 시한·기대값은 유지한다.
- 참고 후보는 [PowerShell SSHProcessMediator](https://github.com/PowerShell/PowerShell/blob/99a49e0eab9b137449b690ab4607d7932c6f9956/src/System.Management.Automation/engine/remoting/server/OutOfProcServerMediator.cs#L526)(MIT), [Win32-OpenSSH fd 초기화](https://github.com/PowerShell/openssh-portable/blob/274893f8d1fe86c4d128b514c3af4bc10cdc0c99/contrib/win32/win32compat/w32fd.c#L132)(구성요소별 BSD형), [cargo-dist PowerShell 설치기](https://github.com/axodotdev/cargo-dist/blob/29704544bb92182e513a2665401fd023e93035a9/cargo-dist/templates/installer/installer.ps1.j2)(MIT)다. 기존 raw stdin·새 핸들 규칙을 유지하고, 경로는 현지 환경에서 선택한다. 외부 코드를 복사하거나 실행하지 않았다.
- 첫 전체 검증에서 기존 `m13::pr_followup`은 시험의 직접 write가 만든 빈 routine과 완성된 잘못된 trigger를 ticker가 각각 읽어 오류 2개를 기록했다. 원본 로그의 서로 다른 두 오류와 `routine_pass`의 내용 hash별 중복 방지를 대조했다. 시험 자료를 `.pending`에 완성한 뒤 `.md`로 rename하여 읽기 경합을 없앤다. 기존 오류 개수·PR 사건·시한은 유지한다.

## 셸 대화 파일 전환의 최신 끝(t-0851)

- 원본 t-0849와 두 번째 t-0850 E2E93의 실패 ID는 휴대폰 WebKit·Claude에서 `switch`한 새 대화다. 원본 실험실과 agent 사건·JSONL은 확인했지만 실패 화면의 DOM·trace는 없어 당시 스크롤 상태까지 소급 확정하지 않는다. 원래 owner를 제품 수정 전 한 번 실행하면 통과할 수 있다. `visible`은 화면 밖에 mount된 본문도 인정하므로 원래 단언만으로 최신 끝 표시를 보장하지 않는다.
- 별도 원인 실험은 구판 웹 번들을 쓰고 같은 PID가 새 파일의 300줄을 먼저 쓴 뒤, 실제 앞 쪽 응답 commit으로 300줄이 합쳐진 것을 확인하고 마지막 60줄을 쓴다. `기록 359`의 원래 30초 visible 대기가 실패하며 실패 DOM은 360개 row shell·최신 본문 없음·top=21800/height=51084/client=665다. 최신 요청은 계속 150줄을 받으므로 전송 미완료가 아니라 옛 읽기 상태와 가상화의 결합이다. 원본 두 실행에는 DOM/전송 trace가 없어 동일한 쓰기 시점까지 소급 확정하지 않는다. 이 원인 실험은 정본 owner와 별도이며 정본의 모든 조합은 유지한다.
- 결정적 회귀는 옛 파일의 첫 줄을 읽고 `scrollTop=0`·scroll 사건을 확정한 뒤 실제 같은 PID로 파일을 전환한다. 기존 `기록 359` visible 단언을 유지하고, 최신 끝과 실제 scroll 위치가 1px 안에서 맞는지도 검사한다. 구판은 새 최신 150줄 뒤 앞 150줄을 자동 요청하고 중간에 남아 실패한다. 기존 30초 locator·20초 DOM 상한·브라우저 조합·재개 단계는 유지한다.
- 390px 양 엔진·양 harness에서는 떠난 동안 새 파일로 바꾼 뒤 복귀도 검사한다. fixture는 첫 300줄 뒤 멈추고 `finish-history` 명령으로 나머지 60줄을 쓴다. 고정 sleep 없이 실제 PID 연결·본문 commit으로 진행하며, 기록을 쓰는 중 감지되어도 최신 끝을 계속 따라가는지 확인한다. 같은 파일의 prepend·append와 맨 위 0 유지·읽던 줄 복귀는 기존 owner가 그대로 검사한다.
- `ChatPanel.commitPage`는 이미 받은 `history`가 바뀌거나, 복귀 기준 줄의 history가 새 파일과 다를 때 기준 줄·복귀·끝 따라가기를 초기화한다. 새 파일의 첫 쪽을 그리기 전에 적용하며 같은 파일의 앞 쪽·새 답변에는 적용하지 않는다. API·cursor 계약·가상화 기준은 바꾸지 않는다.
- 실패 진단은 기존 `ChatDiagnostics`를 E2E93에도 연결해 단계·폭·엔진·harness·전환 전 위치, 요청별 before/history/첫·끝 ID/개수, 실패 DOM·스크롤 높이·화면을 남긴다. fixture의 내용만 쓰며 통과 경로에 파일 쓰기나 별도 대기는 없다.
- 참고 후보: [TanStack Virtual core](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts)의 측정 높이 보정과 끝 따라가기, [react-window List](https://github.com/bvaughn/react-window/blob/master/lib/components/list/List.tsx)의 row key·측정과 이동 경로. [TanStack MIT](https://github.com/TanStack/virtual/blob/main/LICENSE)·[react-window MIT](https://github.com/bvaughn/react-window/blob/master/LICENSE.md)를 확인했다. 측정과 실제 위치를 대조하는 방식을 참고하고 외부 코드는 복사하지 않았다. 새 가상화 도구는 추가하지 않는다.
- 정본은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다. Playwright 1.64.1과 브라우저는 자기 작업 폴더에 격리한다.

## m13 권한 창 경합 fixture와 이미지 완료 표지(t-0850)

- `claude_race_ready_prepares_input_before_publishing_prompt`는 실제 PTY에서 가짜 Claude의 준비 함수를 실행하고, 첫 `key()` 전에 `stty -a`를 수집한다. 구판의 `echo race armed → prompt(stty echo) → key(stty -echo)`는 준비 뒤에도 `echo·icanon`이 켜져 결정적으로 실패한다. 준비 전에 비정규·무반향 모드를 만들고 입력 상자를 한 번에 그린다. 준비 후 화면 변화는 TTY echo나 늦은 입력 상자가 아니라 실제 권한 메뉴다.
- 기존 `claude_thread_states_questions_and_messages`의 `unknown`·`race 1. Yes` 부재·받음 사건 부재·질문/취소 단언을 유지한다. 제품은 글을 쓰기 전과 Enter 전에 화면·질문 상태를 검사한다. 제품의 Enter 전 재검사만 제거한 대조는 수정한 fixture에서도 `race 1. Yes`로 실패했다. 대조 뒤 제품 bytes를 복원했다. 이번 수리는 가짜 raw 입력창의 준비 계약을 바로잡으며, 임의의 실제 프로그램에서 화면 검사 뒤 새 메뉴가 뜨는 모든 순서를 해결했다는 뜻은 아니다.
- `image::probe_completion_is_published_only_after_exit_code_is_written`는 셸 `printf`를 감싸 리다이렉션으로 파일을 연 직후 pipe로 멈춘다. 구판은 이때 최종 표지가 빈 문자열로 보여 실패한다. 임시 파일이 빈 상태에서 최종 표지가 없고, 해제 뒤 종료 코드 7·완성된 출력·임시 파일 제거를 확인한다. 같은 폴더의 임시 파일에 쓰고 닫은 뒤 `mv`로 완료 표지를 공개하며 소비자는 기다림에서 읽은 값 자체를 파싱한다.
- 참고 후보: [Pexpect READY 대기 시험](https://github.com/pexpect/pexpect/blob/master/tests/test_winsize.py)과 [echo 준비 순서 설명](https://pexpect.readthedocs.io/en/stable/commonissues.html), [portable-pty Bash 예제](https://github.com/wezterm/wezterm/blob/main/pty/examples/bash.rs). [Pexpect ISC](https://github.com/pexpect/pexpect/blob/master/LICENSE)·[portable-pty MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)를 확인했다. 준비가 끝난 표지 뒤 입력하고 출력과 종료를 구분하는 방식을 따르며 코드는 복사하지 않았다. Pexpect의 고정 입력 지연은 적용하지 않는다.
- E2E87은 가짜 agent의 `report.md` 생성만 본 뒤 긴 본문을 같은 inode에 쓰면 원래 printf의 남은 출력과 섞인다. 생성은 원래 writer가 파일을 열었다는 조건으로 유지하고, 긴 보고서를 같은 폴더의 임시 파일에 완성한 뒤 rename으로 교체한다. 옛 FD의 남은 쓰기는 교체된 보고서에 닿지 않는다. 공통 가짜 agent·제품 코드는 바꾸지 않는다.
- `node --test web/test/reportFile.test.mjs`는 원래 fixture의 FD를 열어 일부를 쓰고 → 긴 보고서를 공개하고 → 옛 FD가 남은 질문을 쓰는 순서를 고정한다. 구판의 직접 쓰기는 정확한 본문 일치 단언에서 실패하고 rename 뒤에는 통과한다. 고정 sleep·기대값 완화가 없다. E2E87 owner는 Chromium/WebKit·데스크톱/휴대폰·밝음/어두움 8조합의 질문·보고서·thread·할 일·설정 스크롤 및 인증 화면 단언을 그대로 한 번 실행한다.
- 추가 참고는 [write-file-atomic](https://github.com/npm/write-file-atomic/blob/main/lib/index.js)의 close 뒤 rename과 [python-atomicwrites](https://github.com/untitaker/python-atomicwrites/blob/master/atomicwrites/__init__.py)의 같은 파일시스템 내 교체다. [ISC](https://github.com/npm/write-file-atomic/blob/main/LICENSE.md)·[MIT](https://github.com/untitaker/python-atomicwrites/blob/master/LICENSE) 라이선스를 확인했고 코드·의존성은 가져오지 않았다.
- 새 두 m13 경계 시험과 기존 두 owner는 `cargo test --locked -p aaa --test m13`의 정확한 이름 필터로 함께 실행한다. 정본은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`·의존 경계 시험·공개 검사다. E2E87 추가 수리 뒤에는 그 owner만 실행하며 전체 gate의 최종 판정은 묶음 착지에서 한다. E2E93은 별도 후보에서 처리한다. 시한·기대값·재시도·ignore는 바꾸지 않는다.

## 코드 줄 수 시험의 경과 시간 분리(t-0845)

- `nice -n 15 cargo test --locked -p aaa-agent --lib workspace_metrics -- --nocapture`: 실제 Git 객체를 읽고, 집계 규칙·UTF-8 오류·같은 커밋 재스캔 방지·다음 커밋 회복 시험에는 고정 경과 0을 넣는다. 같은 손상 객체에 경과 750ms를 넣으면 시간 상한 오류가 먼저 발생하는 대조도 둔다. Git 자식 실행이 느려져도 오류 원인 시험의 결과가 달라지지 않으며, 오류 단언에는 실제 오류 원문을 남긴다.
- 별도 경계 시험은 749,999,999ns·750ms·751ms와 첫 객체 처리 뒤 750ms 도달을 주입한다. 상한 오류·검사 횟수·기존 성공 스냅샷의 바이트 보존·부분 집계 미저장·동일 커밋 재스캔 방지·다음 커밋 증분 회복을 검사한다. 같은 커밋 재호출에서 경과 함수를 부르면 panic하므로 객체 검사 전에 멈추는지 확인한다. 고정 sleep·상한 연장·ignore·재시도는 없다.
- 제품 입구는 기존과 같이 Git 조회 전에 `Instant::now()`를 만들고 각 객체 읽기 전에 실제 경과 `< 750ms`를 검사한다. 경과 출처만 함수 인자로 분리하며 API·집계 규칙·비용 상한·저장 형식은 유지한다. [원본 Windows CI](https://github.com/cLazyZombie/aaa/actions/runs/39895911745/job/120226199404)에는 UTF-8 단언 실패만 있고 첫 오류 값은 없어 750ms 오류가 원본 실패의 직접 원인이라고 확정하지 않는다. 코드에서 확인한 부하 의존 경로를 제거한다.
- 참고 후보: [tokei 정확도 시험](https://github.com/XAMPPRocky/tokei/blob/master/tests/accuracy.rs)의 fixture 기반 집계 단언과 [scc worker 시험](https://github.com/boyter/scc/blob/master/processor/workers_test.go)의 입력별 직접 결과 단언. [tokei MIT](https://github.com/XAMPPRocky/tokei/blob/master/LICENCE-MIT)와 [scc MIT](https://github.com/boyter/scc/blob/master/LICENSE)를 확인했다. 확인한 집계·시험 경로에서 750ms 상한 시험 예제는 찾지 못했으며, 정확도와 실행 속도를 분리하는 방식만 따르고 코드는 복사하지 않았다.
- 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`·의존 경계 시험·공개 검사다. Windows 실기 실행 여부는 보고에 별도로 남긴다.

## E2E03의 마우스 해제와 키 입력 흐름(t-0845 후속)

- 원본 전체 gate에서 「z 받음」은 실제 `GOT:7:"z\u001b[<0;40;11m"`을 받고도 단일 `GOT === "z"`를 기다려 20초에 실패했다. 둘째 화면 클릭의 마우스 해제를 받은 사건을 먼저 확인하고 첫 화면에서 z를 보낸다. 받은 GOT 조각을 순서대로 이어 전체가 정확한 클릭 누름·해제+z인지 검사한다. 같은 좌표의 SGR 누름·해제 쌍, 최초 클릭의 해제·끌기 무전송, 다른 화면 크기 적용·원래 크기 회복, WebSocket resize→input 순서 단언을 유지한다. 시한·제품 코드는 바꾸지 않는다.
- `node --test web/test/receivedInput.test.mjs`: 해제+z가 합쳐진 정상 조각을 구판의 단일 z 단언이 놓치는 대조를 둔다. CSI 안을 포함한 모든 분할 경계·한 byte씩 분할·중간 SIZE 사건에서도 정확히 같은 흐름을 검사한다. 해제 누락·z 선행·중복/추가 키는 거부하므로 부분 문자열만 찾아 통과시키지 않는다.
- `NODE_PATH=<Playwright 1.64.1 모듈 폴더> PLAYWRIGHT_BROWSERS_PATH=<격리 브라우저 폴더> AAA_BIN=<이번 검증 실행 파일> node web/e2e/web.e2e.mjs --spec 03-keys --workers 1`: 수정 뒤 전체 owner를 한 번 실행한다. 원래 전체 gate의 E2E03·93 실패 원문을 보존하고 전체 gate는 t-0839와 같은 묶음 착지에서 판정한다. 실패 재시도·시한 증가·단언 완화는 없다.
- 참고: [node-pty UnixTerminal 시험](https://github.com/microsoft/node-pty/blob/main/src/unixTerminal.test.ts)의 수신 자료 누적과 [xterm.js parser 시험](https://github.com/xtermjs/xterm.js/blob/master/src/common/parser/EscapeSequenceParser.test.ts)의 한 글자씩 나눈 입력 대조를 따른다. [node-pty MIT](https://github.com/microsoft/node-pty/blob/main/LICENSE)·[xterm.js MIT](https://github.com/xtermjs/xterm.js/blob/master/LICENSE)를 확인했고 코드는 복사하지 않았다.

## 입력 기록 초기화와 파일 보기 재열기 대기(t-0836)

- 원본 [run 39889977347](https://github.com/cLazyZombie/aaa/actions/runs/39889977347)은 CPU 3개·worker 2개·94파일/105작업 중 E2E09의 「보낸 뒤 비움」과 E2E16의 「다시 읽은 뒤 소스」가 각각 20초에 실패했다. 앞 대기는 textarea가 아니라 진단 수집 건수다. 원본 산출물에는 해당 화면의 DOM·요청·사건 기록이 없어 세부 원인을 확정하지 않는다.
- 로컬 worker 2·`AAA_E2E_THROTTLE=4`의 두 owner 전체는 원본 조건과 같은 실패를 재현하지 못했다. 별도 대조에서는 보내기 성공 뒤 `selectionchange`를 넣자 실제 0개 초기화 → 새 1개 수집 → 옛 마지막 0개 대기의 20초 실패를 확인했다. 설정의 실제 `history.go`를 파일 링크 mouseup까지 보류한 대조에서는 viewer 열림 → 늦은 popstate → viewer 제거 → 「다시 읽은 뒤 소스」 20초 실패를 확인했다. 이 대조는 가능한 사건 순서의 증거이며 원본 CI 사건 순서가 같았다는 증거는 아니다.
- E2E09는 보내기 전에 DOM observer를 걸어 실제 0개 표시와 보내기 단추 비활성화를 확인한다. 성공 메시지·파일의 모든 사건과 보낸 글·권한·인증 거부 단언은 유지한다. 그 뒤 새 selection 사건 수집도 검사하므로 초기화를 확인하려고 수집기를 끄지 않는다. 고정 300ms 대신 마지막 음절의 실제 PTY 출력을 기다린다.
- E2E16은 설정 DOM 제거뿐 아니라 설정 history state와 query를 벗어나는 popstate도 기다린 뒤 링크 좌표를 잰다. `node --test web/test/settingsClosed.test.mjs`는 DOM/기록 도착 양쪽 순서·query 잔류·이미 도착한 상태·20초 실패와 정리를 검사한다. 소스 원문 일치·줄 바꿈·기기별 설정·Escape·PTY 무전송 등 기존 단언을 유지한다.
- 단일 [GitHub 진단](https://github.com/cLazyZombie/aaa/actions/runs/39893701999)은 수정 전 두 전체 owner를 각 고정 3회·CPU 3개/worker 2/감속 4로 실행했다. E2E09 3/3, E2E16 2/3 통과이며 원래 두 실패는 재현하지 못했다. 나머지 1회는 HTML의 임의 CSP 사건(`img-src`)을 fetch 차단(`connect-src`)의 완료로 오해한 별개 실패다. fixture는 실제 connect-src 사건만 표지에 남기고 뒤의 img-src 사건에도 보존하는지 검사한다. 기존 기대값과 sandbox 단언은 유지한다. 진단을 재실행하지 않으며 최종 후보의 원격 통과로 해석하지 않는다.
- 실패 시 `diagnostics/input-trace.json`은 건수·성공 표시와 selection/focus 순서를, `diagnostics/file-open.json`은 클릭·기록 이동·viewer/소스 존재·맞춤·파일 요청 단계를 남긴다. 본문·전체 URL·임의 history state는 기록하지 않으며 기존 CI 산출물 경로로 복사한다.
- 참고 후보는 [Playwright의 response owner](https://github.com/microsoft/playwright/blob/main/tests/page/page-network-response.spec.ts)와 [playwright-vscode의 fixture/latch](https://github.com/microsoft/playwright-vscode/blob/main/tests/utils.ts)다. 양쪽 Apache-2.0 라이선스를 확인했고 코드는 복사하지 않았다. 동작 전에 관측을 걸고 실제 상태·사건으로 끝내는 방식과 실험실 격리를 따른다. CI worker 2·20초 대기·120초 단계·40분 job 한도는 바꾸지 않는다.

## 시스템 Zig로 Linux strict cross 검사

- macOS에서는 Homebrew의 `zig`와 Rust의 `x86_64-unknown-linux-gnu` target을 준비한다. `nice -n 15 bash scripts/ci-local.sh --only linux-clippy --tee`는 workspace/all-targets/locked/keep-going과 `-D warnings`로 실제 Linux cfg를 검사한다. 기본 ci-local·GitHub OS별 검사는 유지하며 cross 단계는 명시할 때만 돈다.
- `scripts/ci-linux-clippy.sh`는 현재 PATH 끝에 저장소의 `scripts/cross`를 붙인다. 기존 `x86_64-linux-gnu-gcc`·명시한 `CC_x86_64_unknown_linux_gnu`·`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`를 우선한다. 없으면 작은 GCC 이름 wrapper가 `zig cc -target x86_64-linux-gnu`로 C 빌드와 링크를 연결한다. 셸 rc·전역 PATH·기본 native linker는 바꾸지 않는다.
- wrapper는 `AAA_ZIG` 실행 파일, 현재 PATH의 `zig`, Homebrew 표준 경로 순서로 찾는다. 기존 작업 폴더의 Zig는 `AAA_ZIG=/path/to/zig` 또는 기존 PATH로 계속 사용할 수 있다. cc-rs가 덧붙이는 `--target=x86_64-unknown-linux-gnu`만 Zig 형식 `--target=x86_64-linux-gnu`로 바꾸고 나머지 인자는 유지한다. 명시 경로가 실패하면 다른 판으로 조용히 바꾸지 않고 원래 오류를 반환한다.
- `python3 -m unittest scripts.test_linux_cross scripts.test_ci_local`은 공백·셸 문자 인자의 그대로 전달, Zig 종료 코드 전파, 기존 컴파일러·linker 보존과 선택 단계 집계를 확인한다. 실제 cross clippy와 전체 ci-local도 각각 실행한다. cross clippy는 Linux에서의 실행 시험을 대신하지 않는다.
- 참고: [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)의 [MIT 라이선스](https://github.com/rust-cross/cargo-zigbuild/blob/main/LICENSE), [hermetic_cc_toolchain](https://github.com/uber/hermetic_cc_toolchain)의 [MIT 라이선스](https://github.com/uber/hermetic_cc_toolchain/blob/main/LICENSE). 두 구현의 `zig cc`와 명시 target 전달 방식을 참고하고 코드는 복사하지 않았다. 추가 Cargo 도구나 GCC toolchain 설치 없이 이 검사의 작은 연결만 둔다.

## 셸 pane 재개·복귀 기록과 맨 위 앵커(t-0839)

- t-0845의 E2E93 연결 해제 실패는 agent의 보존 사건에서 `released` 뒤 44ms 후 `session_start(null)`이 남아 host의 agent를 다시 만든 것으로 확정했다. 기록 감시는 앞 PID가 없는 셸 복귀 때 내부 연결만 지우고 foreground의 `released`가 해제를 맡는다. `shell_return_cannot_restart_released_harness_for_either_poll_order`는 Codex·Claude 각각 두 감시 순서를 직접 실행해 host 재연결용 사건이 해제 하나만 남으며 다음 실제 프로세스의 resume도 다시 연결되는지 확인한다. 수정 전 0.04초에 두 사건이 남아 실패했고 수리 뒤 정상이다. E2E93 owner는 원래 20초·`!state().agent`·대화 단추 제거 단언을 유지해 한 번 실행한다.

- `nice -n 15 cargo test --locked -p aaa-agent --lib shell_chat`와 `chat_paths_tests`는 Codex의 `-<thread-id>_<rollout-id>.jsonl`을 열린 쓰기 FD·첫 session_meta와 대조한다. 대화 UUID와 기록 UUID를 구분하고 잘못된 접미사·다른 ID·허용 root 밖·하위 agent·여러 후보를 거른다. 같은 ID의 백업을 열어 고르지 않는 기존 조건을 유지한다. 계정 홈의 hook 경로·ID 조회·metadata 보완도 같은 판정을 쓴다.
- `nice -n 15 cargo test --locked -p aaa-vt`는 고정 입력줄 위 일부 영역의 DECSTBM+LF가 기록과 replay에 남는지 확인한다. 중간 영역·CSI S는 기록을 만들지 않는다. xterm.js 5.5 `BufferService.scroll`·`InputHandler.scrollUp`의 판정을 따른다. 기존 구현은 첫 기록 유실 단언에서 실패했고 수리 뒤 정상이다. backend와 웹의 alternate 화면·커서·마우스 모드 경로는 유지한다.
- `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs --spec 93-shell-chat --workers 1`: 기존 직접 실행 8조합에 연결 전 alternate 화면 복귀·대화 보기 선택·읽던 첫 메시지 복귀를 더한다. 새 owner는 두 엔진·1280px/390px에서 inline 터미널 기록 복원, Codex resume·Claude resume/continue의 이전 360메시지·입력창 고정, 프로그램의 마우스 보고 스크롤과 고정 footer, agent 재시작 뒤 실제 PID의 재연결을 확인한다. 재시작 전에 자기 실험실의 harness.json만 지워 저장한 ID의 재전송과 구분한다. 기존 byte cursor는 재시작에 무효가 되므로 새 cursor로 이전 기록 전체를 읽고 복귀를 검사한다.
- Chromium 휴대폰은 CDP의 실제 touch를 쓴다. Playwright WebKit 휴대폰에는 touch 이동 API가 없어 TermView에 touch 사건을 보내는 경로만 확인하며 실제 iPhone 조작과 구분한다. 사본의 그리기 표지(`data-view-base`) 뒤 첫 휠도 WebKit에서 버려져 viewport=0·ydisp=43으로 남는 결함을 재현했다. xterm의 자체 동기화 무시 표지는 같은 줄의 사건에만 적용한다. `xterm-source.test.mjs`는 실제 upstream 메서드에 같은 patch를 붙여 자체 동기화·합쳐진 사용자 스크롤·reset의 0 높이를 대조한다. native scrollTop만 보고 화면 이동 완료로 간주하지 않는다.
- E2E93의 안내 높이 대조는 처음 메시지까지 읽고 scrollTop=0에서 안내 padding을 정확히 1px 늘린다. 기존 구현은 0→1px로 결정적으로 실패했다. 첫 줄을 고정하던 앵커가 안내 높이 변화를 따라가던 결함이며, 맨 위와 중간/앞 페이지 추가를 나눠 0을 유지한다. 기존 touch→scrollend→scrollTop=0→두 frame→정확히 0 단언도 유지한다. reset 경계의 JS 쓰기·stack·안내/첫 row 위치를 기록해 실패 때 diagnostics/shell-scroll-reset.json에 남긴다. t-0838의 통과한 대조 10회에는 쓰기가 없었고 원래 실패에는 계측이 없어, 이 대조만으로 원래 실패의 변경 주체를 소급 확정하지 않는다.
- 사용자 설정의 `tui.alternate_screen="never"`는 Codex 자체의 고정 입력줄 스크롤을 끈다. thread와 셸은 같은 TermView·마우스 전달·ChatPanel을 사용하며 제품이 전역 설정을 바꾸지 않는다. 실제 사용자 pane의 키 입력·종료 없이 metadata만 대조한다. 정본은 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 시한·단언·브라우저 조합을 줄이지 않는다.
- 전체 Rust 검증에서 report_copy의 경고 대기 실패가 났고, 남은 DB는 새 adopted thread가 `host가 기동 전에 중단됨`으로 실패했음을 보였다. `adoption_preparation_is_not_a_crashed_launch`는 agent 준비 응답을 oneshot으로 보류하고 그 사이 실제 ticker를 한 번 실행한다. 기존 구현은 0.54초에 Starting→Failed로 실패한다. thread_insert transaction 안에서 기존 작업 claim을 얻어 준비 완료까지 유지한다. `nice -n 15 env -u AAA_SESSION_ID cargo test --locked -p aaa --test report_copy`는 새 경합과 기존 보고·library·불변 판 저장 실패/동일 hash 복구를 함께 검사한다. sleep·시한 연장으로 피하지 않는다.

## GitHub 웹 E2E 직렬 실행과 파일 보기 Escape(t-0830)

- 공개 `macos-15` arm64 러너는 [GitHub 사양](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)에 따라 3 CPU다. 10 CPU 미만인 기존 로컬 기본값 `floor(cores / 2)`는 worker 1이 된다. CI만 `AAA_E2E_WORKERS=2`로 명시하고 로컬 기본과 `--workers` 우선순위는 유지한다. 모든 spec·브라우저·viewport·독립 단계는 그대로 한 번씩 실행한다.
- 원본 run `39805550736`의 SHA `4fe3e4ad`와 run `39879814485`의 SHA `ef5de571`은 spec 머리가 번호순이며, 다음 머리까지의 시간은 spec 실행·정리를 포함한다. 전자는 88번까지 2,161.8초, 후자는 83번까지 2,219.1초를 썼다. 마지막 시작 파일을 정지한 파일이라고 해석하지 않는다.
- `ef5de571`의 Playwright 설치는 25초, 웹 빌드는 15초, aaa 준비 빌드는 약 138초였다. E2E 자체가 약 37분 동안 직렬로 누적돼 job의 40분 상한에 도달했다. 37-chat-pages 142.9초·39-phone-back 121.5초·23-composer-attachments 103.4초 등이 이어졌으며, 첫 브라우저 실행이나 83번 하나의 정지를 가리키는 자료는 없다. job 상한은 40분을 유지한다.
- 입구는 CPU 수·worker 수·OS/arch·spec/job 수와 각 작업의 시작/끝·슬롯·경과 시간·종료 결과를 실시간으로 출력하고 `AAA_E2E_ARTIFACTS/run-timings.jsonl`에 남긴다. 병렬 출력의 `## specs/…`는 작업 완료 때 묶어 출력되므로 시작 순서로 읽지 않는다. CI는 통과·실패 모두 실행 로그·빌드 로그·단계 요약·시간 기록과 실패 진단을 산출물로 보존한다.
- 원본 E2E16의 「기본 Markdown Escape」 실패에는 xterm의 `input` 메시지 `\u001b`와 PTY의 `^[` 출력이 함께 남았다. 파일 내용은 focus rAF보다 먼저 표시될 수 있고 터미널 부착도 focus를 옮긴다. FileViewer의 window bubble listener는 xterm textarea가 전파를 멈춘 키를 받지 못했다. document capture의 layout effect로 등록해 실제 키를 먼저 처리하고 기본 동작·전파를 막는다. 이미 처리한 키·IME·상위 native modal·그림 크게 보기는 먼저 둔다.
- `NODE_PATH=<Playwright 1.64.1 모듈 폴더> PLAYWRIGHT_BROWSERS_PATH=<격리 브라우저 폴더> node web/e2e/web.e2e.mjs --spec 16 --workers 1`: 기존 전체 owner에 실제 textarea focus 경계·Chromium·WebKit 소스와 Markdown의 Escape 닫힘·터미널 무입력·상위 native 창과 그림 크게 보기 우선순위를 더한다. 구판은 textarea를 focus한 대조에서 원본과 같은 20초 닫힘 실패와 ESC 전송으로 실패한다. 원본의 모든 부착·focus 사건 순서까지 재현했다고 주장하지 않는다.
- 단일 진단 [run 39885298949](https://github.com/cLazyZombie/aaa/actions/runs/39885298949)은 CPU 3개·worker 3개를 확인했다. 93파일·104작업을 모두 한 번 실행해 E2E 1,203.645초·job 23분 59초에 끝났으며 E2E16은 62.171초에 통과했다. 유일한 실패는 E2E66의 네 기기 줄바꿈 단계가 120초를 넘긴 것이다. 최종 설정은 worker 2개로 여유를 두며, 이 설정의 원격 판정은 착지 뒤 main CI에 남긴다. 이 실패를 통과로 바꾸거나 진단 job을 재실행하지 않는다.
- E2E66의 당시 캐시 최적화는 t-0928에서 제거했다. 현재는 native 줄 경계 전후 표본마다 독립 native textarea를 새로 만들고 실제 줄 수·폭·focus·미전송을 대조한다. 양 엔진·양 화면과 120초 상한을 유지하며 자세한 축소·회귀 소유는 t-0928 절을 따른다. 실패 때 `diagnostics/wrap-geometry.json`에 현재 기기·진행 상태·직전 측정과 완료 기기별 비용을 남긴다.
- 실패하면 `diagnostics/file-viewer-escape.json`에 실제 키의 target 종류·터미널 여부·isTrusted·defaultPrevented·viewer/history 유무와 ESC 누출 여부를 남긴다. 본문·주소·임의 history state는 기록하지 않는다. 기존 닫힘 단언·20초 대기·120초 단계 상한·실패 판정을 유지한다. 정본은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사이며, GitHub 확인은 진단 branch의 단일 job 또는 착지 뒤 main CI로 한다. 실패 재실행·시한 증가·기대값 완화는 없다.

## 셸에서 직접 실행한 Codex·Claude 기록 연결(t-0832)

- `nice -n 15 cargo test --locked -p aaa-agent --lib shell_chat`: 실제 OS에서 자기 PID가 쓰기로 연 기록만 연결하고, 더 새 미개방 파일·여러 CLI 후보·하위 agent·root 밖 symlink를 거르는지 확인한다. Claude PID 등록의 다른 PID·오래된 시작 시각·background·잘못된 UUID·중복 프로젝트 기록 거부, 교체·해제·기존 hook 중복 방지를 검사한다. 운영 홈·설정·인증 파일은 읽거나 바꾸지 않는다.
- `nice -n 15 cargo test --locked -p aaa-host --lib shell_harness_replacement`: 같은 pane의 다른 실행이 옛 세션 ID·Claude 기록 경로를 이어받지 않고 셸 복귀에서 모두 해제되는지 확인한다.
- `chat_paths_tests::codex_chat_uses_the_exact_bound_file_even_when_the_id_has_a_backup_copy`는 같은 UUID의 두 파일을 각각 정확한 경로로 고를 수 있고, root 밖 경로·다른 ID·잘못된 UUID는 hint로 쓰지 않는지 검사한다. E2E93에도 같은 ID의 미개방 백업을 두어 실제 쓰기 파일을 읽는지 확인한다.
- `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 93-shell-chat --workers 1`: 실제 기본 셸에서 hook을 호출하지 않는 Python harness를 실행한다. Codex는 실제 쓰기 FD를 유지하고 Claude는 실제 PID 등록만 남긴다. 연결 전 읽기 오류, 늦은 첫 기록 뒤 자동 연결, 앞 페이지부터 360줄 전체, 갱신 때 읽던 위치, 같은 PID의 세션 전환, 종료 뒤 셸 복귀를 확인한다. 데스크톱 Chromium/WebKit과 390px 두 엔진을 쓰며 Chromium 휴대폰은 CDP의 실제 touch 입력·scrollend, mobile WebKit은 Playwright wheel 미지원 때문에 DOM scroll 경로를 확인한다. 실제 iPhone 인수와 구분한다.
- 새 시험 준비에서 화면 밖 메시지의 본문은 가상화로 mount되지 않는다는 것을 확인했다. 갱신은 실제 응답의 메시지 ID와 row shell commit으로 기다린다. 터치 뒤 관성 스크롤은 scrollend로 끝내고, 맨 위로 돌린 뒤 두 native frame에서 가상 row의 위치 측정을 끝낸 다음 갱신을 검사한다. 고정 sleep·재시도·시한 연장·제품 기대값 변경은 없다. 정본 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사·의존 경계 시험을 수행한다.

## 예약의 앞 할 일(t-0829)

- `nice -n 15 cargo test --locked -p aaa-host --lib task_queue::tests::reservation_after`: 없는 앞 할 일·자기 지정·순환 거부, 앞 할 일 완료 대기 중 뒤 예약의 자리 판정 진행, 완료 체크만으로는 보관 전까지 대기, 보관 정리 뒤 차례, 완료 없이 지운 앞 할 일의 확인 필요, host 재시작 보존과 이미 보관된 앞 할 일을 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 92-task-after --workers 1`: 실제 host·agent에서 웹 예약 확인의 「앞 할 일」로 예약하고 tick 뒤 대기 까닭·thread 없음을 본다. 갱신 확인이 지금 앞 할 일을 고른 채 열리는지, CLI 순환 예약 거부를 본다. 앞 할 일을 `--done` 하고 tick 한 번에 보관·실제 기동을 host 기록으로 확인한다. WebKit 390px에서 확인창 가로 넘침이 없고 진행 중인 앞 할 일 뒤 예약이 기다리는지 본다. 고정 sleep·재시도·시한 연장은 없다.
## iPhone 뒤로 미리보기 수리 철회(t-0805 후속)

- 376a344c 배포 뒤 5회와 bcf2abaf 뒤 2회 모두 Safari 탭·홈 화면 앱에서 끄는 동안만 이전 메뉴가 보이고 손을 놓으면 사라졌다. 원문 JSON 파일은 없으며 사용자 기록 요약과 답을 근거로 한다. 처음의 「계속 열림」 버튼은 「미리보기만」으로 정정됐다. 앱 DOM은 open=false/right=0이며, 사용자 캡처에도 이전 항목 쪽 메뉴와 밀려 나가는 현재 대화가 보인다. 강제 DOM 숨김과 두 rAF 대기는 이 증상에 효과가 없었다.
- 사용자 10-23 「쉽게 고쳐지는게 아니면 원래대로 수정해」에 따라 이동을 지연하던 `drawerNavigation`과 전용 시험, 이전 주소 A를 첫 frame까지 보존하도록 요구하던 E2E 실험 단언을 제거했다. 기록 전 class 제거·강제 layout도 제거했다. 이 실험은 Safari 스냅숏 완료를 검증하지 못했으며 유지할 제품 요구가 아니다. 실패를 재시도하거나 시한을 늘려 통과시키는 변경이 아니다.
- 메뉴 선택은 기존처럼 기록·본문을 바로 바꾸고 React 상태로 닫는다. `inList`가 해당 pane 행의 aria-current=true를 기다리는 준비 경계, E2E39의 A→메뉴로 B→Back과 모든 목적지·양방향 이동·설정 내부 복귀·넓은 화면의 기존 단언은 유지한다. 취소·이동·복원 뒤 늦은 터치로 재개방되지 않는 유닛/E2E도 유지하며, 앱 이동 중 쓸기 취소와 취소 불가능한 move를 검사한다.
- E2E39 `fragment-history.mjs`는 Chromium·WebKit의 실제 location.hash→popstate/hashchange, Back, B 재선택 때 전체 칸 수 유지, history.pushState가 popstate를 만들지 않음을 대조한다. 원본은 실험실 diagnostics/fragment-*.json에 남긴다. 직후 popstate를 Back이라고 단정하지 않으며 history.length는 현재 위치가 아니다.
- 진단 v2의 layout commit/passive effect·기록 메서드와 쓰기 전/후·익명 목적지 번호는 유지한다. 기본 탭에서는 수집하지 않는다. 정본 ci-local·공개 검사와 E2E39 전체로 되돌림을 검증한다. Safari 기본 뒤로 미리보기는 미해결로 남기며 추가 실험 배포·실기 반복은 이번 범위에서 진행하지 않는다. API·E2E 번호는 유지한다.


## E2E39의 Chromium 기록 목록 비교(t-0866)

- t-0865의 진단에서 첫 pane 선택 뒤 길이는 3, `#threads` 4 → thread 5 → report 6 → reports 7이다. 다음 서로 다른 `#tasks`에서도 7이고, question 8 → asks 9 → 설정 목록 10 → 모양 11로 한 칸씩 작게 관측된다. 첫 Back의 popstate에서만 12로 보정되며 그 사이 `pushState`·`replaceState` 호출은 없다. 이미 준비한 열 목적지와 시작의 두 칸을 더하면 12다. 순회의 모든 목적지는 제자리걸음 없이 정확히 복원되므로, 끝에서 제품이 중복 항목 하나를 만든 실패가 아니라 시작 길이의 낡은 관측값을 기준으로 삼은 시험 결함이다.
- Chromium은 `Page.getNavigationHistory`로 브라우저의 실제 항목 수·현재 위치·항목 ID와 목적지를 순회 전후 비교한다. 같은 수의 항목으로 바뀌어도 실패한다. WebKit은 기존 native `history.length` 비교를 유지한다. 진단의 순회 시작/끝에는 실제 목록을 함께 넣되 전체 URL·pairing query·임의 state는 저장하지 않는다. 모든 목적지·메뉴·설정·시트 단언과 사건 대기·시한은 그대로다.
- 기존 fragment 탐침의 독립 페이지에서 Chromium renderer의 길이만 한 칸 작게 고정하고 실제 Back/Forward 뒤 native 값으로 돌린다. 옛 길이 비교는 실패하지만 브라우저 항목 목록은 같음을 확인한다. 이어 실제 pushState의 증감, 같은 수로 forward 항목을 바꾼 분기, replaceState의 목적지 교체를 거부한다. WebKit도 실제 추가·감소를 거부한다. 이 대조는 관측 결함을 결정적으로 검사하며 원본 Chromium 내부 IPC 경합의 재현은 아니다.
- [Chromium renderer](https://chromium.googlesource.com/chromium/src/+/main/content/renderer/render_frame_impl.cc)의 `PrepareFrameForCommit`은 commit 인자의 기록 위치·길이를 WebView에 적용한다. 보고서 frame의 늦은 commit/취소와 겹칠 수 있는 구조는 확인했지만 원본에는 browser IPC trace가 없어 그 세부 시점은 확정하지 않는다. 새 진단은 renderer 값과 브라우저 목록을 함께 남겨 두 층을 구분한다.
- 참고 후보는 [Playwright history 시험](https://github.com/microsoft/playwright/blob/main/tests/page/page-history.spec.ts)의 실제 엔진·주소 왕복, [Vue Router](https://github.com/vuejs/router/blob/main/packages/router/src/router.ts)의 같은 목적지 중복 이동 판정, [React Router history](https://github.com/remix-run/react-router/blob/main/packages/react-router/lib/router/history.ts)의 항목 key·index 구분이다. [Playwright Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE), [Vue MIT](https://github.com/vuejs/router/blob/main/LICENSE), [React Router MIT](https://github.com/remix-run/react-router/blob/main/LICENSE.md)를 확인했다. 실제 항목을 비교하는 방식을 따르고 코드·의존성은 가져오지 않는다. 제품에 중복 push 방지를 추가할 근거는 없다.
- 해당 owner 전체는 `NODE_PATH=<Playwright 1.64.1 모듈> PLAYWRIGHT_BROWSERS_PATH=<격리 브라우저> AAA_BIN=<자기 빌드> nice -n 15 node web/e2e/web.e2e.mjs --spec 39 --workers 1`로 확인한다. 묶음 착지 때 전체 ci-local을 실행한다. 실패 재시도·시한 연장·기대 길이 변경은 하지 않는다.

## E2E39 기록 수 실패 진단(t-0825)

- `history-destinations.mjs`는 제품이 시작하기 전에 `pushState`·`replaceState`와 native `popstate`·`hashchange`, iframe load를 기록한다. 각 사건에 경과 시각·length·hash·설정 쪽/깊이·시트 표시를 남기고, 순회 시작/끝과 메뉴 상태·방향·목적지를 함께 보존한다. pairing query·전체 URL·임의 history state는 기록하지 않는다.
- 마지막 비교의 기대/실제 length와 사건 순서를 실패 메시지에 넣는다. 순회 도중 실패해도 실험실의 `diagnostics/history-destinations-<engine>.json`에 기록을 저장하므로 기존 CI 산출물 수집에 포함된다. 증가·감소를 모두 실패로 유지하며 기존 목적지·본문·메뉴 단언, 시한과 사건 대기는 바꾸지 않는다.
- 기준 be26951f에서 로컬 E2E39 전체 1회, 캡처 없는 목적지 순회 1회, 고정 8회/worker 3의 목적지 순회, 전체 ci-local과 원래 실패 머신의 동일 binary E2E39 전체 고정 3회/worker 3이 모두 통과했다. 별도로 보고서 응답을 다음 본문 이동까지 보류한 순서와 Chromium CPU 8배 감속도 각각 통과했다. 모든 수집본의 시작/끝 length는 12이고 순회 중 제품의 기록 쓰기는 없었다. 이 결과는 원본 실패 재현이나 원인 수리의 증거가 아니다.
- 원본에는 기대/실제 length와 쓰기 순서가 없어 제품 결함·시험 경합·브라우저 관측 차이를 확정하지 못했다. 진단을 포함한 새 실패 자료가 필요하다. 제품을 추측해 바꾸거나 성공한 실행으로 원래 실패를 덮지 않는다.
- 작업 폴더의 별도 복사본에만 마지막 `pushState`·`replaceState`를 주입한 대조는 기대 12/실제 13으로 실패하며 두 호출의 순서와 CI용 JSON 복사를 확인했다. 이 주입은 제품·최종 시험에 넣지 않으며 원본 결함 재현으로 해석하지 않는다.

## restart 시험의 FDA 주입 환경 격리(t-0821)

- `nice -n 15 cargo test --locked -p aaa --test restart -- --nocapture`: FdaGate의 시험용 `posix_spawn` interpose가 `session-host` 자식을 열 때만 `DYLD_INSERT_LIBRARIES`·`AAA_TEST_FDA_GATE`를 제거한다. worker 자신의 환경과 실제 재시작 exec는 유지하며, 같은 worker PID의 라이브러리 로드·TCC open 진입을 재시작 전후 각각 1회·2회 확인한다. 제품 코드·launcher·실제 TCC DB·시스템 설정은 바꾸지 않는다.
- fixture constructor가 라이브러리 로드 PID를 기록한다. 기존 pane과 재시작 뒤 새 pane 각각의 실제 session host PID가 기록에 없어야 하며, pane shell에도 두 변수가 없어야 한다. 보호된 `/bin/sh`가 DYLD 변수만 지우는 경우도 host 로드 검사로 잡는다. 격리 전에는 같은 owner의 새 단언이 session host 상속으로 3.47초에 실패했다. 원래 시험은 로컬 macOS 27.0.1에서 통과했으므로 이 대조를 macOS 15 CI의 즉시 종료 재현으로 해석하지 않는다. 그 실패와의 인과 및 실제 판정은 착지 뒤 main CI에서 확인한다.
- 두 pane 모두 `wait_for`로 실행 상태를 확인하고 시험 전용 셸 prompt 출력을 기다린 뒤 명령을 보낸다. 화면의 줄 끝 공백 제거와 무관한 `aaa-restart-ready>`를 쓴다. 기존 명령·출력·책임 PID·기존 host PID 보존·승인 거부·detached 단언은 유지한다. 시한 연장·재시도·고정 sleep·판정 완화는 없다.
- 실패하면 정리 전에 pane 목록·화면, `meta.json`·`exit.json`·`final.json`, session host·host·worker 로그와 FDA 로드/진입 PID를 CI 출력에 남긴다. session token이 든 `spec.json`은 출력하지 않는다. `AAA_RESTART_TEST_LAUNCHER`로 지정하는 기존 launcher 경로도 같은 owner로 확인하며, 정본 로컬 검증은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다.

## 종료된 coordinator와 상단 시작 안내(t-0816)

- `node --test web/test/roster.test.mjs`: 종료된 coordinator가 트리·기타·프로젝트 머리·이전/다음 탐색에서 빠지고, 중단·복원 대기·끊긴 머신의 기록과 thread에 연결된 pane은 유지하는지 확인한다. 입력 pane 목록은 바꾸지 않는다. 명령 팔레트도 같은 `roster`를 쓴다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 91-coordinator-exit --workers 1`: 실제 host·agent·PTY에서 Ctrl+C·정상 종료, 종료 기록 보존·새로 열기·기본 profile 재시작을 확인한다. Chromium·WebKit 각각 1280×844·390×844에서 상단 안내 위치·passkey 공존·메뉴를 열지 않은 시작·가로 넘침·보기 권한을 검사한다.
- 시작 요청은 실제 host로 전달한다. 첫 요청의 profile만 허용 밖 값으로 보내 host의 실제 거부를 안내줄에 보이는지 확인하며, 다음 요청의 전달을 사건으로 보류해 시작 중 단추 잠금과 중복 요청 차단을 검사한다. 제품 상태·종료 결과를 가짜로 만들지 않는다. 새 sleep·시한 연장·재시도는 없다. 이 headless 휴대폰 폭 검사는 실제 iPhone 조작을 대신하지 않는다.
- 관련 기존 E2E26·12와 전체 `nice -n 15 bash scripts/ci-local.sh --tee`, 공개 검사를 수행한다. API·link 번호는 유지하며 E2E91·plan §8.460은 착지 때 main과 대조한다.
- E2E15의 안내 없는 머리줄 검사는 실제 coordinator를 띄워 전제를 준비한다. 기존 0px·안전 영역·겹침 결함 심기 단언을 유지한다. 설정을 열면 시작 안내는 숨겨 설정에서 앱 목록으로 나가는 Tab 순서(E2E06)를 유지하며 E2E91에서도 숨김·닫은 뒤 복귀를 확인한다.


## CI 사건의 전달·실행 기록 경합(t-0814)

- `nice -n 15 cargo test --locked -p aaa-host --lib ci::tests::`: 기존 내구 outbox 시험에서 자동화 완료를 먼저 확정하고 같은 source 사건을 다시 전달한다. 실행 결과·시각·last_result·오늘 횟수 전체가 그대로여야 한다. 수정 전에는 `coordinator 안내`가 `CI failed · 자동화 입력`으로 덮여 결정적으로 실패했다. 큐 인계만으로 완료를 기록하지 않는지도 확인한다.
- `nice -n 15 env -u AAA_SESSION_ID cargo test --locked -p aaa --test m13 ci::`: 첫 실패와 바뀐 실패 시험 각각의 자동화 실행 완료를 기록으로 기다린다. 뒤의 304 두 번과 tick은 CI 판정의 무변화 확인에만 쓴다. 첫 실패와 새 사건의 최종 결과 일치, 재시작 중복 방지, 같은 실패 집합 무깨움, 실패 시험 변경 시 다시 깨움, 회복·조회 장애의 기존 단언을 유지한다.
- 자동화로 넘긴 사건의 대기·실패·완료는 실행부가 기록한다. 감시만 한 결과·같은 실패 집합·옛 목적지 변환 대기는 source가 계속 기록한다. 웹 실행 기록은 같은 ID의 결과·시각을 그대로 그리므로 source 재전달로 실행 결과를 덮지 않아야 한다. API·웹 코드·E2E 번호는 바꾸지 않는다.
- 공용 watch의 Git owner도 큐에 있는 입력의 완료 기록·오늘 횟수가 없고 자기 착지의 건너뜀은 남는지 확인한다. 같은 head 재조회는 기록 전체가 같아야 한다. 기존 Git m13도 실제 동작 완료 뒤 재시작하므로 재시작 전후 기록 비교에 실행 중 상태가 섞이지 않는다.
- 고정 sleep·재시도·시한 연장·기대값 완화 없이 기존 owner를 강화한다. 정본 검증은 전체 `nice -n 15 env -u AAA_SESSION_ID bash scripts/ci-local.sh --tee`와 공개 검사다.

## E2E88 WebKit 새 창 종료 원인과 시험 도구 갱신(t-0800)

- macOS 15 arm64 진단 run `39793997341`은 같은 E2E88을 새 실험실에서 고정 3회 실행해 모두 WebKit desktop의 HTML 링크 클릭에서 실패했다. 클릭은 성공했지만 popup 사건 전에 브라우저가 exit 139로 끝났다. `page.close`·`context.close`·`browser.disconnected`가 시험의 명시적 정리보다 앞선다. 두 OS crash 기록은 새 창 생성의 `WebPageProxy::useGPUProcessForDOMRenderingEnabled()+344`, EXC_BAD_ACCESS/주소 0x40을 가리킨다. 동일 설치 binary의 해당 명령도 주소 0x40 읽기다. `page.crash` 사건이 없다는 이유로 브라우저 전체 crash를 배제하지 않는다.
- 공식 [WebKit bug 317666](https://bugs.webkit.org/show_bug.cgi?id=317666)과 [수정 68455aeaffed](https://github.com/WebKit/WebKit/commit/68455aeaffed)은 관련 페이지 순회에서 포인터를 비운 뒤 다음 증가식이 읽던 결함을 고친다. 수정된 순회를 포함한 Playwright 1.64.1/WebKit 27.2로 같은 macOS 15의 run `39795685013`에서 E2E88 전체 조합이 고정 3회 모두 통과했다. 이 두 run은 실패 재실행으로 녹색을 고른 것이 아니라, 구판 0/3과 수정판 3/3의 대조다. 진단 workflow는 최종 후보에 넣지 않는다.
- `clickForPopup`은 기다림과 클릭의 개별 결과를 기록한다. E2E88 실패의 `diagnostics/video-popup.json`에는 시각·단계·양쪽 결과와 page crash/close·context close·browser disconnected, 명시적 정리 시작을 남긴다. 실제 브라우저의 의도한 클릭 오류/브라우저 선종료 대조와 `node --test web/test/popup.test.mjs`는 늦은 결과도 보존하고 두 종료 순서를 구분함을 확인한다. 주입 대조를 본래 결함의 재현으로 쓰지는 않는다.
- 초기 포인터 준비가 충분하다는 판정은 7회차 통합 gate에서 반증되어 제거했다. 세 Mac의 실측 커널은 모두 `uname -r=27.0.0`이며 OS는 host 27.0, 원격 데스크톱·노트북 27.0.1이다. 기존 커널 major 26 조건은 어느 곳에도 맞지 않았다. 동일한 앱 없는 input+button 탐침에서 두 데스크톱은 rAF 0·일반 click 실패, 노트북은 통과했다. 실제 `mouse.move(0,0)` 사건이 전달돼도 두 데스크톱의 정지는 풀리지 않았다. 창 모드·full headless·viewport·focus·VSync 타이머 변경도 해결하지 못했다. 조사 때 두 데스크톱의 물리 화면은 asleep/inactive, 노트북은 awake/active였으나 화면 전원만을 직접 원인으로 단정하지 않는다.
- 실패한 Chromium 156 GPU 자식은 CPU time 0이며 dyld의 `prepare → makeLaunchLoader → mapFileReadOnly → __open`에서 멈췄다. renderer trace는 frame sink 생성을 요청했지만 BeginFrame이 없었다. 실제 책임자 조회에서 GPU는 자기 책임자이고 browser와 달랐다. [Chromium의 `MacDisclaimGpuTccResponsibility`](https://chromium.googlesource.com/chromium/src/+/refs/tags/156.0.8078.4/content/browser/gpu/gpu_process_host.cc#154)는 이 GPU 자식만 `posix_spawn` 때 책임 계보를 끊는다. 그 feature를 끄면 GPU는 browser와 같은 책임자를 유지하고 초기화·실제 rAF·일반 click이 정상이다. GPU를 in-process로 바꾸는 대조도 통과했지만 최종 수리는 별도 GPU 프로세스를 유지한다. OS 내부의 open 정지 기전까지 입증했다고 주장하지 않는다.
- `chromium-launch.mjs`는 macOS 시험 브라우저와 showcase에만 위 책임 분리를 끄는 옵션을 준다. OS 판 조건·포인터 준비·화면 깨우기·권한/launchd 설정 변경은 없다. `--disable-features`를 추가하면 기존 목록이 덮이므로 Playwright 1.64.1의 목록을 함께 보존한다. E2E01은 실제 `launchServer().process().spawnargs`의 기본 목록과 비교해 갱신 때 누락을 막고, 새 브라우저의 앱 없는 input+button에서 서로 다른 세 native rAF·일반 locator click의 isTrusted·정확한 입력값을 검사한다. 최초 keeper의 CDP 인자 조회가 enable-automation 없이 거부된 fixture 오류는 공개 launchServer의 실제 argv 조회로 고쳤다. automation flag를 켜거나 제품 단언을 바꾸지 않는다.
- 최종 실행 옵션의 최소 탐침은 세 Mac에서 미리 정한 3회씩 모두 통과했다. 비교한 Playwright 1.64.0-alpha-2026-10-01(Chromium155, WebKit2369의 upstream은 1.64.1과 동일)와 1.65.0-alpha-2026-10-22(Chromium156.0.8078.12)도 host의 기본 실행에서는 실패했다. 구 Chromium147은 동일 host에서 정상이고, 설치된 Chrome156.0.8078.47은 정상이나 같은 판의 CfT full/headless는 실패했다. 따라서 다른 Playwright 판이나 다른 실행 파일로 교체하지 않고 1.64.1의 원래 번들 Chromium·WebKit을 유지한다. force click·rAF 대체·시한 증가·브라우저 제외·실패 재시도는 없다.
- WebKit 27의 `font-family` 직렬화는 공백 있는 가족 이름의 따옴표를 생략한다. E2E06은 고정한 기대 CSS를 독립 요소의 CSSOM으로 직렬화한다. 기존 가족 이름·순서·generic·기본 fallback·저장 원문·실제 글자 폭 단언은 보존하며 실제 속성에서 기대값을 만들지 않는다. 수정 전 같은 설정 owner가 따옴표 차이로 실패했고 수정 뒤 전체 owner가 통과했다.
- 수정판 첫 전체 gate는 Rust 전체를 통과했지만 웹 92파일 중 E2E01·10이 실패했다. E2E10은 정확한 키에 pointer/touch 사건만 오고 click이 없었다. 최소 input+button 대조에서도 pointerdown 취소 시 click이 사라지며, mousedown만 취소하면 click과 입력 focus가 보존된다. KeyBar의 pointerdown 취소만 없애고 원래 mousedown focus 보호·반복·이동/취소 추적을 유지한다. E2E03의 WebKit 실제 tap keeper는 Escape와 기본 왼쪽 방향키의 정확한 bytes·터미널 focus를 검사한다. 원래 binary에서는 새 Escape keeper가 실패했다. 첫 keeper에서 기본 막대에 없는 Backspace를 고른 fixture 실수는 기본 방향키로 바로잡았다. 원래 Chromium hold/cancel·modifier·IME와 E2E10의 iPad/iPhone tap·키보드/PTY 단언도 모두 유지한다.
- PWA는 첫 frame과 별개다. Chromium 156에서 offline 상태로 SW가 준 문서를 다시 열면 요청은 막혀도 `navigator.onLine`이 이미 true여서 복구 시 native online 사건이 없었다. 앱 listener는 이미 등록돼 있었다. 요청 차단/복구는 기존 `ctx.setOffline`을 쓰고, [공식 CDP `Network.overrideNetworkState`](https://chromedevtools.github.io/devtools-protocol/tot/Network/#method-overrideNetworkState)로 새 문서의 실제 navigator 상태도 맞춘다. offline의 false와 복구 native online의 `isTrusted`·true, 원래 token gate 복구를 단언한다. 앱 handler·API 응답·브라우저 사건을 JS로 가짜 호출하지 않는다. E2E01·03·10 전체 owner는 수리 뒤 3/3 통과했다.
- CI와 설치 안내의 Playwright는 1.64.1이다. ci-local도 이 판을 검사하고, 격리 설치는 명시한 `NODE_PATH`와 `PLAYWRIGHT_BROWSERS_PATH`를 쓴다. 전역 설치·시스템 설정·브라우저 캐시는 바꾸지 않는다. 정본 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사이며, 원래 E2E88의 네 조합·sandbox 경계·직접 MP4/Range·답한 bytes 보존을 유지한다. headless 결과를 실제 Safari/iPhone의 출력 장치 검증으로 해석하지 않는다.

## 메뉴 선택 뒤 휴대폰 쓸기의 진단과 정리(t-0805)

- 사용자 순서는 대화 A → 메뉴에서 B 선택 → 왼쪽 가장자리 뒤로다. 초기 진단본은 두 메뉴 선택 모두 history-write 순간 open=true·오른쪽 경계 343px를 확인했다. 이 DOM 경계만으로 Safari OS 미리보기의 원인을 확정하지 않는다. 강제 숨김·두 frame 대기의 철회와 현재 검증 범위는 위 후속 절을 따른다.
- `drawer-navigation.mjs`는 Chromium·WebKit 390px에서 위 순서, x25의 빠른 쓸기 후 touchcancel만 도착한 경우, native popstate 뒤 늦은 move/end와 move/cancel, pagehide/pageshow 사건 경계를 검사한다. 메뉴 class·inline transform·실제 경계가 닫혀야 한다. Touch 생성은 WebKit의 `document.createTouch/List`와 Chromium의 생성자를 각각 쓴다. 설정 내부 되감기·모든 목적지·390/430px·홈 화면 앱 흉내·넓은 화면의 기존 E2E39 단언은 유지한다. 실제 bfcache 복원이나 iPhone OS 몸짓을 합성 사건으로 입증했다고 쓰지 않는다.
- 유닛 `node --test web/test/edgeSwipe.test.mjs`의 빠른 취소·popstate·pagehide·pageshow 네 회귀는 원래 구현에서 모두 메뉴 재개방으로 실패했다. 설정 내부 되감기와 정상 안쪽 flick·기존 손가락 추적은 유지한다. 시한 연장·실패 재시도·단언 완화·고정 sleep은 없다.
- 실기 진단은 앱 주소 query의 `drawer-debug=1`로 켠다. A → 메뉴로 B → 가장자리 뒤로 뒤 정상/미리보기만/계속 열림을 표시하고 기록을 저장할 수 있다. 과거 9회 확인 절차를 추가 사용자 요청으로 취급하지 않는다. 이번 되돌림에서 새 실기 반복은 요청하지 않는다.
- `?drawer-debug=1` 없는 탭에는 진단 UI·사건 수집이 없다. 진단은 최근 800개 사건과 전체 개수만 탭 메모리에 두며 URL·history.state·본문·인증 값을 기록하지 않는다. DOM 경계가 닫혔는데 사용자에게 미리보기만 보이면 OS 스냅숏 후보로 분류한다. 화면 합성 결과는 DOM 로그만으로 단정하지 않는다. 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다.

## 시작 자가 검사와 agent 종료 분리(t-0808)

- `nice -n 15 cargo test --locked -p aaa-agent --lib selfcheck:: -- --nocapture`: 실제 FIFO open은 writer를 열기 전까지 완료될 수 없다. 검사 진입 사건 뒤 runtime drop 완료를 먼저 요구한다. 실패 때도 writer를 열어 자기 thread를 회수하고 원래 실패를 보고한다. 기존 spawn_blocking에서는 종료 관측이 실패했고 전용 thread로 분리한 뒤 통과했다. runtime이 사라진 뒤 검사를 풀어도 디스크 결과·실패 이유·다음 hello 요약이 보존되는지 확인하며, 진행 중에는 이전 완료 결과를 유지한다.
- 기존 `nice -n 15 cargo test --locked -p aaa --test restart -- --nocapture` owner에서 시험 전용 DYLD interpose로 FDA의 시스템 TCC DB open만 실험실 FIFO로 바꾼다. 실제 TCC DB·시스템 설정·제품 환경 분기는 바꾸지 않는다. worker PID가 기록한 open 진입 뒤 writer를 닫아 둔 채 실제 service restart로 launcher·worker PID·기존 pane·새 pane·책임 PID·승인 거부·detached 검사를 유지한다. exec 뒤 같은 worker의 두 번째 open 진입을 확인하고 writer를 열어 정상 FDA 결과 저장까지 확인한다. 같은 최종 owner에 옛 spawn_blocking 경로를 다시 심으면 기존 30초의 새 worker 준비 기록 대기가 실패하고, sample에서 runtime drop → blocking pool 종료 대기와 FDA probe → FIFO open을 함께 확인했다.
- 새 launcher 기본 경로와 `AAA_RESTART_TEST_LAUNCHER`로 지정한 기존 등록 launcher의 읽기 전용 경로를 모두 실행한다. launcher는 운영 서비스를 재시작하지 않는 별도 실험실 자식으로 쓴다. 시한 연장·실패 재시도·기대값 완화·고정 sleep은 없다. 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.
## 실행 파일 축소와 panic 콜스택(t-0804)

- `cargo test --release --locked -p aaa-common --lib panic_log::`는 네 역할에서 실제 panic을 일으킨다.
  함수 이름이 있는 콜스택·원래 위치/횟수/task ID·unwind 후 생존·stderr 잠금 중 `latest()` 관측을 확인한다.
  payload·동적 이름·환경·절대 빌드 경로 비노출 단언은 유지한다. 프레임 파일은 저장소 상대 경로 허용 목록 또는 `external`뿐이다.
  release workflow도 다섯 native target에서 이 owner를 실행한다. Windows PDB가 필요한 해석을 교차 컴파일 성공으로 대체하지 않는다.
- `cargo test --locked -p aaa-host --lib web::assets::`는 내장된 모든 자산을 실제 `web/dist` 원문과 바이트 대조한다.
  실제 static handler로 gzip/identity·명시적 `q=0` 거부·여러 헤더·wildcard·MIME·cache·404·SPA fallback을 확인한다.
  gzip 응답을 강제로 없애거나 거부를 무시하는 구현은 같은 기대값에서 실패한다. 웹을 빌드하지 않은 기존 안내도 유지한다.
- `python3 -m unittest scripts.test_release scripts.test_deploy_build`는 PDB/dSYM의 독립 SHA와 대응 실행 파일 SHA,
  심볼 누락 거부, macOS unpacked 지정·dSYM 생성/복사, 기존 bootstrap·launcher 결속·provenance를 검사한다. 최종 파일을 후처리하지 않아 서명/SHA 순서가 바뀌지 않는다.
- 크기는 같은 소스의 release 후보별로 기록한다. 프로세스 이름을 `aaa`로 맞추고 빌드가 끝난 뒤 기준/후보를
  번갈아 실행하며 stdout 원문 일치와 CLI 벽시계·CPU 시간을 측정한다. 첫 실행은 따로 두고 나머지 30개 중앙값을 사용한다.
  HTTP는 새 config/state·빈 port·가짜 tailscale·격리 usage home의 실제 host를 쓰며 listen 로그 사건으로 준비를 판정한다.
  원문과 gzip을 풀어 원래 자산 bytes와 비교하고 자기가 띄운 PID 하나의 종료를 기다린다. 고정 sleep이나 실패 재시도는 없다.
- macOS 결과·Windows/MSVC 및 Linux/musl 교차 링크·native CI를 구분해 기록한다. 로컬 정본은 전체 ci-local·deps·공개 검사다.
  Linux 압축 DWARF의 실제 함수·줄 해석과 macOS의 dSYM 동반/단독 panic 출력을 각각 확인한다.
  dSYM 존재·UUID 일치만으로 충분하지 않다. kache와 packed의 object 누락을 확인해 unpacked로 보존했다.
  실측과 심볼 보관 조건은 [크기 분석](binary-size.md)을 따른다. FRONT_API·AGENT_LINK·E2E 번호는 바꾸지 않는다.

## 새 launcher 아래 restart fixture 격리(t-0798)

- `nice -n 15 cargo test --locked -p aaa --test restart -- --nocapture`는 등록되지 않은 새 launcher를 기본으로 쓴다. `AAA_RESTART_TEST_LAUNCHER`로 기존 launcher를 지정하는 읽기 전용 경로도 유지한다. 두 경로 모두 기존 launcher/worker PID·기존 pane 보존·새 pane·책임 PID·승인 거부·detached 재관측 단언을 그대로 검사한다.
- 원본은 보호 볼륨의 Python 스크립트를 open하다가 `probe-before`의 30초 시한으로 실패했다. 같은 새 책임자 아래 Apple·Homebrew Python 모두 일반 임시 폴더의 같은 스크립트는 즉시 성공하고 보호 볼륨에서는 정지했으며, 해당 PID의 TCC `SystemPolicyAllFiles` 거부를 확인했다. 자료만 옮긴 대조는 다음으로 `kache rustc -vV`에서 보호 볼륨의 사용자 store를 열다가 멈췄다. 시험 전용 `KACHE_CACHE_DIR`를 준 대조는 실제 cargo/build.rs까지 통과했다. Python 판이나 CPU 부하로 단정하지 않는다.
- fixture 자료·cwd·생성 실행 파일·Cargo target·kache store는 사용자 캐시 아래 실행마다 새로 만든 전용 폴더에 둔다. kache wrapper는 유지하고 전역 설정·기존 store·TCC·launchd 등록을 변경하지 않는다. Cargo target을 OS 임시 폴더나 다른 worktree에 공유하지 않는다. 완료된 각 단계의 원본 결과는 시험 책임자가 자기 worktree의 `target/restart-probes`로 복사하고, 성공하면 전용 캐시를 지우며 실패하면 경로를 남긴다.
- 새 launcher는 기존 C 소스를 그대로 쓰고 별도 시험 신원의 Info.plist·adhoc 서명으로 정상 번들을 만든다. 준비 전에 launcher가 종료되면 바로 실패한다. 진단 하위 명령은 실제 stdout/stderr와 종료 코드를 남기고 완료 출력 사건으로 끝낸다. 정상 출력·시한·기대값을 완화하거나 실패한 명령을 재실행하지 않는다.
- 조사 중 별도 실행에서는 syspolicyd의 EMFILE·YARA 검사 오류와 launcher 종료를 기록했다. 또 다른 cargo 실패의 같은 시각에도 해당 시스템 오류가 있었지만 당시 숨겨진 cargo stderr가 없어 그 실패의 직접 원인은 확정하지 않는다. 정상 번들·격리는 시스템 검사 오류 전체의 수리라는 뜻이 아니다. 이후 실패는 추가된 종료·compiler 오류와 원본 시스템 로그를 함께 확인한다.
- 첫 전체 검증은 재시작 전 진단 뒤 새 worker 준비 기록의 30초 대기에서 한 번 실패했다. 그 실행에는 정리 전 스택이 없어 원인을 확정하지 않는다. 해당 대기에 실패하면 아직 자기 실행 파일을 가리키는 worker·launcher만 sample하고 원래 실패를 다시 전파하도록 진단을 추가했다. 진단본의 단독·병렬 Rust·Rust/웹 동시 전체 검증은 통과했지만 앞 실패가 수리됐다는 증거로 쓰지 않는다. 다음 재발의 스택을 확보한다.
- 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 제품 코드·API·E2E 번호·시스템 권한은 바꾸지 않는다.

## ci-local --tee 병렬 출력(t-0797)

- `--tee`도 기본 실행과 같은 `test e2e clippy` 병렬 경로를 쓴다. 세 명령은 각각의 로컬 로그에 stdout/stderr를 쓰고,
  별도 reader가 `[단계]`를 붙여 화면에 출력한다. 로그 원문·단계 결과·실패 요약·GitHub annotation은 유지한다.
  web→fmt→build와 E2E 실행 사본 준비를 먼저 끝내고, 병렬 단계와 reader가 끝난 뒤 public을 실행한다.
  고른 병렬 단계가 하나면 기존 tee 경로를 쓴다. reader 실패도 전체 실패지만 검증 명령은 모두 끝까지 실행한다.
- `python3 -m unittest scripts.test_ci_local -v`는 실제 셸과 격리 도구 자식 세 개를 socket으로 만난다.
  세 자식이 동시에 진입하고 종료를 허락받기 전에 각 로그가 화면에 도착해야 한다. 고정 sleep 없이 socket과 출력 사건으로 끝낸다.
  한 단계 실패 뒤 나머지 완료·public 후행·원문 로그·개행 없는 마지막 바이트·이전 완료 표지 제거·단일 단계·tee 없는 경로·
  빌드 실패의 의존 단계 건너뜀과 reader 실패를 검사한다. UTF-8이 읽기 경계에서 나뉘어도 깨지지 않고 완료 뒤 큰 로그를 모두 읽어야 한다.
  직렬 분기를 되심으면 동시 진입 대기에서 실패한다. 이 keeper는 Unix의 정본 test 단계에 포함한다.
- 전후 비교는 같은 worktree와 머신에서 준비 빌드를 끝낸 뒤 같은 선택 단계·환경·nice 값으로 차례로 실행한다.
  stage 시간 합과 실제 병렬 구간의 벽시계를 구분하고, 준비 빌드·캐시·주변 부하 차이를 절감액으로 단정하지 않는다.
  최종 정본 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사를 수행한다. 검증 범위·worker·시한을 바꾸지 않는다.

## FD 고갈 시험의 Front 연결 준비(t-0791)

- 두 번째 묶음 gate에서 기존 hard64 시험은 웹 EMFILE을 기록했지만 단발 Front status 요청이 이미 성공해 Front EMFILE 대기가 실패했다. 원본 host 로그·요청 stdout·OS fd 목록을 보존했다. 이 owner와 host accept 구현은 후보 3개에서 바뀌지 않았다.
- 웹 부하에 이어 실제 Unix Front socket 80개도 EOF 전까지 유지한다. 시작·ticker 등 임시 fd 반환 때 단발 요청 하나가 통과하는 틈을 지속 연결로 채우며, Front accept의 실제 EMFILE을 계속 필수로 요구한다. 두 부하를 닫은 뒤 fd 회수·새 Front 요청 성공·시스템 경고 한 건·웹 HTTP 복구의 기존 단언과 15초 상한을 유지한다. 운영 한도·제품 accept·재시도·기대값은 바꾸지 않는다.

## 작업 목록 요약의 키보드 준비 경합(t-0791)

- 묶음 착지 첫 전체 gate의 E2E28은 메뉴의 「요약」에 focus한 뒤 Enter를 보냈지만 상세가 열리지 않았다. 기존 RowMenu는 첫 항목 「보고」의 focus를 다음 animation frame에 예약한다. 기준 main 36f3e935의 격리 사본에서 그 콜백을 「요약」 focus 직후 전달하면 focus가 「보고」로 돌아가고 실제 click도 「보고」로 발생하며 같은 상세 대기가 실패한다.
- E2E28의 기존 키보드 경로만 메뉴 초기 frame과 첫 항목의 focusin 완료를 기다린 뒤 「요약」으로 옮긴다. 고정 sleep·시한 연장·기대값 변경 없이 기존 Enter·같은 작업·주소 보존·Reader·휴대폰 검사를 유지한다. 후보를 빼지 않으며 기준 main의 시험 준비 경합 수리로 별도 기록한다.

## 설정 닫기와 휴대폰 메뉴의 통합 경합(t-0780, t-0771)

- 기존 E2E39의 `historyDestinations`를 t-0771과 통합한 코드에서 그대로 실행해 `pane-bk2`의 30초 click 실패를 재현했다. 설정의 작업 패널 선택 → `recordSettings([])`의 내부 되감기 → popstate 메뉴 강제 닫기 순서이며, 도우미가 없는 실제 조작에서도 메뉴가 사라지는 제품 결함이다.
- E2E39의 `settings-drawer-history.mjs`는 `history.go`의 전달만 보류한다. 실제 설정 기록 모듈이 되감기를 요청한 사건을 기다리고, 사용자가 메뉴를 연 상태에서 native go를 풀어 실제 popstate·설정 상태·drawer transition으로 끝낸다. 설정에서 작업 패널을 고른 경우와 X로 닫은 직후 다시 메뉴를 연 경우를 Chromium·WebKit 390px에서 확인한다. 도우미로 재개방하지 않고 pane 줄을 직접 눌러 선택할 수 있어야 한다.
- 복귀 분류는 해당 사건에만 적용한다. 이어서 메뉴를 열고 사용자가 `history.back/forward`하면 두 방향 모두 닫혀야 하며, 기존 목적지 순회의 설정 목록·쪽 단언도 유지한다. 수정 전 Chromium의 작업 패널 선택 경로에서 「브라우저 이동 없이 설정 내부 복귀가 사용자가 연 메뉴를 닫았다」로 실패했다. 시한·단언 완화·고정 sleep·실패 재시도는 없다. 실제 OS 가장자리 몸짓은 headless 합성 터치·history 이동과 구분한다.
- 정본 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 후보 commit 뒤 공개 검사다. API·E2E 번호는 유지한다.

## m13 자동화 CI 준비 대기와 실패 요약(t-0787)

- main `212d8425`의 macOS 실패 원문(run `39768260021`, job `119789031781`)은 `30s 안에 labA 연결이 되지 않았다`다. `ci.rs`의 FIFO·batch·ack 단계에 들어가기 전 `world_build`에서 실패했다. 같은 실험실의 host/agent 로그는 약 1초 안에 연결 성공을 기록했다. 당시 `machine list` 응답·오류와 CLI 실행 시간은 남아 있지 않아 과거 실패의 직접 원인은 확정하지 않는다. 변경 전 해당 시험의 로컬 단독 실행은 통과했다.
- 공통 준비 단계는 agent를 띄우기 전에 Front 상태 사건을 구독하고 `labA`의 실제 `Connected`를 기다린다. CLI 반복 기동·조회 오류를 `None`으로 버리는 경로에 준비 판정을 맡기지 않는다. 30초 상한과 자동화의 기존 단언·FIFO·ack 순서는 유지한다.
- `cargo test -p aaa --test m13 agent_readiness_uses_initial_state_and_reconnection_events -- --exact`는 실제 host/agent에서 구독 전의 현재 연결, 종료 사건, 구독 뒤 재연결 사건을 확인한다. 연결 대기 실패에는 마지막 머신 목록·host/agent 로그를 붙이고, Front 수신 오류는 원문 이유를 남긴다. 진단용 추가 CLI 조회는 하지 않는다.
- 공통 준비를 쓰는 async N-1 호환 시험은 동기 `world()`를 `spawn_blocking`에서 준비한다. 첫 전체 검증에서 감시용 runtime 중첩이 드러났고 이를 수리한 뒤 같은 호환 시험이 통과했다. 해당 시험의 연결·profile 거부 단언은 유지한다.
- `python3 -m unittest scripts.test_rust_tests`는 실제 자식 runner가 실패 코드·모든 시험·doctest를 보존하면서 긴 진단 뒤에도 `FAIL <묶음> <시험>: <panic 첫 줄>`을 로그 끝과 `summary.json`에 남기는지 검사한다. 원본 runner에서 새 keeper는 `failures` 누락으로 실패했다. 시한·기대값 변경이나 실패 재실행으로 넘기지 않는다.
- 관련 회귀는 `cargo test -p aaa --test m13 ci::`, 정본 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 새 macOS CI에서 같은 실패가 나면 추가된 상태·수신 오류·로그로 원인을 좁힌다. 로컬 통과만으로 과거 러너의 원인까지 입증했다고 하지 않는다.

## 질문·보고 영상 재생(t-0767, plan §8.448)

- t-1155 회귀는 기존 E2E88의 네 조합을 확장한다. 주소 없는 첫 화면은 기본 controls가 꺼져 있고 첫 재생 단추만 있어야 한다. 첫 클릭·decode·탐색 뒤에는 controls가 켜지고 자체 단추가 사라져야 하며, Chromium 기본 아이콘·WebKit 중앙 재생 영역의 실제 포인터 클릭으로 일시정지 위치보다 시간이 전진해야 한다. `play()`를 대신 호출하거나 가짜 media를 쓰지 않는다. 새 대기는 timeupdate/error로 끝나며 실패 상한은 5초다. 필요한 이유: 기존 시험은 자체 단추만 눌러 주소 없는 기본 재생과 일시정지 때 겹치는 단추를 잡지 못했다.
- 같은 네 조합의 실제 host Range 응답은 206·video/mp4·Accept-Ranges뿐 아니라 Content-Range·Content-Length·무압축·원본 두 바이트 일치까지 확인한다. 수정 전 실행본은 controls=true·src 없음·readyState=0에서 새 단언에 실패했다. 수정 후 캡처 포함 네 조합은 단계당 2.1~3.1초에 통과했다. WebKit의 Space 키는 기본 재생을 시작하지 않고, 좁은 컨트롤 막대의 좌표는 되감기를 누를 수 있어 실제 재생 영역을 사용한다. Safari 앱·실제 iPhone 검증과 headless WebKit 검증은 구별한다.
- `nice -n 15 cargo test --locked -p aaa-host --lib web::`: 기존 인증 경로를 cookie 없이 열었을 때 세 종류 모두 401·UTF-8 JSON·한글 원문인지 확인한다. 새 첨부 cap의 재사용·만료·기기 폐기·보고서 주소 변환 거부와 리소스 CSP의 정확한 폴더 제한을 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 88-report-video --workers 1`: 실제 host·agent·publish·coordinator post·thread 보고를 쓴다. 외부 자료 없는 2초 H.264 High/yuv420p/faststart 영상은 `web/e2e/fixtures/report-video.mp4`다. Chromium·WebKit 각각 1280×800·390×844에서 본문과 선택지의 첫 재생 단추·metadata 수신·재생 뒤 실제 decode·시간 전진·seeked·소리 없음·인라인·자동 재생 없음·답 선택 보존·가로 넘침을 검사한다.
- 같은 질문의 HTML 링크는 `/v/…/library/…/index.html#A`다. iframe과 새 탭의 실제 영상 decode, cookie 없는 직접 MP4·206 Range·MIME·Accept-Ranges, HTML의 자기 파일 GET·cookie/API/다른 cap 차단, 보고 Markdown의 재생, 새 보고 뒤 답한 영상 원본 bytes를 확인한다. coordinator AGENTS.md가 질문 문법을 `aaa guide questions`로 가리키는지, 그 주제와 thread brief에 영상 문법이 있는지도 확인한다(§8.491). 앱 API 인증·sandbox를 풀거나 영상 객체·play를 가짜로 만들지 않는다.
- E2E53의 웹 `files_base`는 `/v/<48자리 hex>/`여야 한다. 답 내용·원문·사본 보존 검사를 유지하고, cookie 없는 읽기의 MIME·전체 바이트가 원본 그림과 같은지도 확인한다.
- 완료 사건은 host 보고 hash·DOM·loadedmetadata/timeupdate/seeked다. 실패 뒤 시한 연장·재시도 없이 원인을 조사한다. 관련 E2E21·59·69·74와 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사로 회귀를 확인한다. headless WebKit은 실제 macOS Safari·iPhone의 출력 장치·UI를 보증하지 않는다.

### E2E88 실패 진단(t-0786)

- HTML 링크의 popup 대기와 실제 click은 `clickForPopup`에서 함께 관측한다. click이 먼저 실패한 뒤 정리 과정이 context를 닫아도 popup의 늦은 거부를 처리하므로 원래 오류가 남는다. `node --test web/test/popup.test.mjs`는 클릭 전 대기 등록·클릭 완료, 클릭 실패 뒤 context 종료, popup 실패 뒤 늦은 클릭 실패를 확인한다. 옛 순서의 구현을 별도 사본에 심으면 뒤 두 시험이 unhandled rejection으로 실패한다.
- 실제 WebKit desktop의 HTML 링크 클릭만 의도적으로 거부한 별도 사본에서도 대조했다. 옛 순서는 CI와 같은 popup 종료 unhandled rejection으로 죽고, 새 순서는 최초 클릭 오류·단계·본문/iframe 영상 상태를 `FAIL`에 남긴다. 두 사본 모두 실패로 끝나며 영상 재생과 sandbox 검사는 실제 제품을 쓴다. 오류 은폐의 재현이며 최초 CI 클릭 오류의 원인을 재현한 것은 아니다.
- 네 조합 각각의 실패에는 단계, 선택값, 주소, 영상 readyState·currentTime·paused·오류와 최근 click·hashchange·media 사건을 붙인다. 선택 보존과 주소 보존은 독립 단언이며 재생·탐색 실패에도 실제 상태를 남긴다. 영상 재생·시한·sandbox·제품 기대값은 그대로다.
- `212d8425`의 macOS CI는 WebKit desktop의 iframe 확인 뒤 popup 대기 거부로 종료돼 최초 오류를 남기지 못했다. t-0783 첫 gate의 선택/주소 복합 단언도 두 값을 기록하지 않았다. 로컬 macOS 27·Playwright 1.59.1의 캡처 포함 10회·캡처 없는 10회는 모두 통과했다. 최초 원인과 두 실패의 관련성은 미확정이며 제품 수리 완료로 해석하지 않는다. CI와 같은 캡처 없는 실행도 반드시 포함한다. 진단 반복은 실패를 통과로 바꾸는 재시도가 아니며 실패한 회차를 모두 보존한다.


## 본문 끝단 스크롤 격리(t-0766, plan §8.447)

- E2E87은 실제 격리 host·agent의 긴 질문(그림 16개·표), Markdown 보고서·thread 보고, 긴 할 일 상세와 단축키 설정을 연다. Chromium·WebKit 각각 1280×720·390×844, 밝음·어두움의 8조합을 검사한다. 질문의 lazy 그림은 각각 화면에 보인 뒤 decode 완료를 기다린다.
- 본문의 높이 제한·가로 넘침, 데스크톱에서 위·아래 끝의 실제 wheel 수신과 렌더링 사건 뒤 문서 좌표·visualViewport·앱·왼쪽 목록·본문 틀의 위치 및 목록 scrollTop 보존을 확인한다. 끝단에서도 본문 scrollTop은 끝에 남아야 한다. 휴대폰은 실제 overflow의 scrollTop과 scroll 사건으로 이동·양 끝을 확인한다. Playwright mobile WebKit은 wheel/swipe를 지원하지 않으므로 손가락 관성 스크롤을 재현했다고 주장하지 않는다.
- headless WebKit은 macOS Safari의 OS 고무줄 효과를 온전히 재현하지 못한다. 따라서 스크롤 전달 차단과 앱이 있는 html·body의 overflow 잠금도 함께 단언한다. ccc4a985 원본 번들에서 질문의 전달 차단 누락으로 RED를 확인한다. 로그인 화면에는 문서 잠금을 적용하지 않으며 기존 E2E01·06과 입력·키보드·viewport 회귀를 유지한다.
- 고정 sleep·시한 연장·실패 재시도 없이 DOM·그림 decode·wheel·렌더링 사건으로 끝낸다. `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 87 --workers 1`과 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행한다.

## 설정 왼쪽 메뉴·목록 행 겹침(t-0771)

- E2E06은 설정 메뉴가 데스크톱의 공통 사이드바에 있고 오른쪽 본문 전체를 내용만 쓰는지 검사한다. Chromium 1280/980/320px·WebKit 1280/390px의 밝음/어두움에서 테마·색상·밀도 조작의 실제 경계와 hit target을 확인하고 캡처한다. 기존 주소·새 탭·새로고침·뒤로/앞으로·Esc·키 바·폰트·휴대폰 메뉴 → 내용 검사를 유지한다.
- 설정 메뉴가 본문 밖으로 이동했으므로 메뉴 locator는 설정 navigation을 사용한다. inList는 앞 패널을 숨긴 상태를 감지하고 실제 활동 막대로 전환한다. 제품 동작을 대신하지 않는다.
- E2E26의 긴 목록과 E2E34의 상태가 다른 자동화 행에서 자식 글·번호가 행 안에 있는지, 다음 행과 겹치지 않는지, 선택/hover 배경이 앞 행을 덮지 않는지 검사한다. 자동화 번호의 상대 시작 위치도 같아야 한다. 답할 것·보고서·할 일·기록·설정 메뉴도 같은 경계 검사를 사용한다.
- 고정 sleep·재시도·시한 연장 없이 DOM·host·scroll 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사다.
- 기존 빌드에서 E2E34의 자동화 행 세로 넘침을 재현했고 공통 행 높이 수리 뒤 E2E06·26·34가 통과했다. E2E06 마지막 단계는 초기 pane 선택 전 주소를 저장하는 경합이 있어 실제 terminal과 pane hash가 준비된 뒤 기준 주소를 기록한다. 복귀 주소 기대값·시한은 유지한다.
- 전체 웹 회귀의 E2E03은 iPad 820px에서 본문 안의 단축키 tab을 찾던 locator 한 곳이 남아 실패했다. 앱 drawer 기준에 따라 왼쪽 navigation에서 찾도록 갱신한 뒤 E2E03 전체가 통과했다. 키 전달·설정 저장·터미널 누출·크기 단언은 그대로다.

## 첨부 뒤 여러 줄 입력의 높이 안정(t-0772, plan §8.452)

- 전체 검증에서 기존 m13 `routines`가 DB 항목을 본 직후 아직 쓰는 중인 inbox Markdown을 빈 내용으로 읽어 실패했다. DB insert→파일 write→안내 action 완료 기록 순서를 확인했고, 파일 단언 전에 `routine runs`의 `done`/`coordinator 안내` 완료를 기다리도록 시험만 고쳤다. 명령 source 종료는 안내 action 종료보다 앞이므로 완료 신호로 대신 쓰지 않는다. 원래 파일 본문·승인·중복 출력 단언과 30초 상한은 유지한다.

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 66-composer-wrap --workers 1`: 기존 줄 경계·게이지 폭 회수·창 폭 변경 검사를 유지하고, 실제 coordinator의 그림 업로드·경로 삽입·빈 줄·여러 줄 한글 입력을 추가한다. Chromium·WebKit 각각 1280px·390px에서 「맨 아래로」가 떠 있는 대화와 짧은 글/높이 상한을 넘는 긴 글을 검사한다.
- OS 입력기를 직접 조작하는 대신 두벌식 자모→음절→확정 사건을 재생하고 trusted 영문 키 입력도 실행한다. 입력 중 높이·화면 위치·내부 scrollTop·상자 높이·문서 스크롤을 기록한다. 원래 scrollHeight getter가 이미 강제하는 layout 직후를 관측하므로 화면 frame 사이의 일시적 축소도 검출한다. ResizeObserver·scroll 사건과 두 requestAnimationFrame 뒤 표본을 함께 보며 고정 sleep·실패 재시도·시한 연장은 없다.
- 수정 전에는 네 조합의 짧은 글과 긴 글 모두 측정 순간 높이가 40px로 줄어 실패한다. WebKit의 긴 글 진단은 240→40→240px와 화면 위치 왕복을 보였다. headless에서 최종으로 그려진 frame의 시각적 떨림까지 재현했다고 주장하지 않는다. 수정 후 같은 시험에서 높이·스크롤·focus·조합 글 보존·편집 중 전송 없음과 삭제 뒤 한 줄 복원을 검사한다.
- 화면과 JSON 표본을 캡처 폴더에 저장한다. E2E23의 첨부 전송·E2E35의 보내기 높이·기존 E2E66을 포함한 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. API와 E2E 번호는 유지한다.


## 작업 요약 카드의 실행 모델(t-0751, plan §8.445)

- `node --test web/test/summaryCard.test.mjs`: 관측 모델·생각 수준의 표시, profile과 실제 모델의 불일치, 미확인 값과 옛 host, 알려진 계열의 짧은 이름·알 수 없는 이름 보존을 검사한다.
- `nice -n 15 cargo test -p aaa-agent --lib model_observation --locked`: 실제 증분 JSONL reader로 Codex sol→astra·effort 변경·누락, 프로세스 시작 전 관측 제외, Claude 본 대화만 선택·생각 수준 추측 없음·하위 agent 제외를 검사한다.
- `nice -n 15 cargo test -p aaa-host --lib plan_events_set_counts_without_changing_the_activity --locked`: 실제 host 상태에 모델을 적용하고 idle 보존·미확인 사건·새 세션의 초기화를 확인한다. 기존 계획·컨텍스트·배경 수·상태 단언을 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 pane 안 hook→agent JSONL reader→Agent link→host→열린 카드의 sol high→astra medium 변경과 같은 상세를 확인한다. /model 선택 결과는 harness 기록만 대신 쓰며 모델 판정·상태 전달·표시는 제품을 쓴다. 재시작의 새 profile·관측 없음, Chromium 데스크톱·320px Reader와 WebKit 390px Reader에서 밝음/어두움·가로 넘침을 검사하고 캡처한다.
- DOM·host·기록 사건으로 끝내며 고정 sleep·실패 재시도·시한 변경은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.109·Agent link 1.11·기존 E2E 번호를 쓰고 착지 때 main의 번호를 다시 확인한다.

### t-0751 검증 환경 진단

첫 전체 실행은 load 97.60~103.38에서 계정 버전 확인·이미지 버전 확인 등 5초 상한의 준비 세 건이 실패했다. 같은 binary·기대값·시한으로 해당 세 건을 차례로 분리 실행해 3/3 통과했다(load 8.05). 첫 실행의 실패 원문은 보존한다.
설치된 Playwright 1.59.1/WebKit 26.4는 macOS 27에서 브라우저 초기화 중 `InspectorPlaywrightAgent`의 SIGBUS가 발생했다. thread 전용 폴더의 Playwright 1.64.1과 Chromium 156/WebKit 27.2로 비교 검사했다. 새 WebKit의 CSS font-family 따옴표 정규화가 기존 E2E06 문자열 검사를 실패시켰고, 지정된 1.59.1에서는 같은 E2E06이 그대로 통과했다. 기대값은 바꾸지 않고 최종 웹 회귀 검증은 지정된 1.59.1·worker 2개로 수행한다. 전역 설치·시스템 설정은 바꾸지 않는다.



## 공개 포트 변경·저장 링크 이전(t-0749, plan §8.444)

- `nice -n 15 cargo test -p aaa-host --lib web_publication --locked`: 실제 DB·원장·router를 사용한다. 옛 포트 해제/다른 서비스 보존, post 본문·refs/본문 SHA/원본 봉투 보존/원래 입력 재전송, inbox 파일 투영, 구판·옛 origin 구독 정리와 loopback 구독·기기 신원 보존을 확인한다. Serve 등록 실패 뒤 host 재개방·같은 대상만 재개, 다른 서비스 충돌·읽기 오류·권한 거부를 검사한다. DB의 구독 삭제에 실패를 주입해 저장 주소·원본 보존·답 상태·공개 주소·구독이 함께 롤백되는지 확인한다.
- `nice -n 15 cargo test -p aaa-host --lib tailscale:: --locked`: 기존 exact/LocalAPI/서비스 보존 owner와 함께 깨진 Serve JSON·없음과 `null`의 구별을 확인한다. 상태 읽기 실패로 후보 포트를 고르거나 기존 공개 주소를 지우면 안 된다.
- `nice -n 15 cargo test -p aaa --test m13 public_port_change --locked`: 실제 CLI→host→Serve 외부 명령→DB→HTTP로 자동 공개·명시 포트 변경·post/refs 이전·기존 cap의 새 주소에서 200·옛 Host 421·다른 매핑 보존·host 재시작 내구성을 검사한다. Python fixture는 Tailscale 실행 파일만 대신하고 제품의 전환·원장·공유 서명·HTTP 입구는 대체하지 않는다. 이 owner는 Unix m13이며 실제 daemon/TLS·Windows 실행의 증거는 아니다.
- CLI 인자 owner는 `public_port_change_parses_and_rejects_off_and_invalid_ports`다. 고정 sleep·실패 재시도·시한 연장 없이 명령 완료·실제 상태·HTTP/DB를 완료 사건으로 쓴다. 정본 전체 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`·공개 검사이며 Linux 교차 clippy도 확인한다. 실제 운영 host/Serve는 변경하지 않는다.
- 결함 심기: `public_origin_in`의 관측 표지 검사를 제거하면 `failed_transition_survives_restart_and_resumes_only_the_same_target`가 「매핑 없음 확인 뒤 옛 포트 fallback 금지」로 실패한다. 원본 bytes를 복원한 뒤 정본 전체를 검증한다. 이전 전 원본 보존은 해당 post 본문 정리·inbox/답 기록 삭제 수명을 함께 따른다.
- 질문 본문 URL 변경은 해시도 바뀐다. `moving_a_question_body_preserves_its_ids_and_partial_answers`는 원래 q-ID 두 개·한 질문의 부분 답을 유지하고 새 본문 해시에 연결해야 한다. revision 이전 전에는 `q-1002/q-1003 != q-1000/q-1001`로 실패했다. 첫 전체 검증에서 `[]`를 struct의 빈 sequence로 읽은 Serve 판정과 새 DB 시험의 불필요한 clone lint도 잡혔다. JSON root를 object/null로 제한하고 같은 기대값·시한을 유지한다.

## 설정 본문·passkey 줄·긴 목록 스크롤(t-0748, plan §8.443)

- E2E06의 새 「설정 본문·passkey 긴 값」 단계는 실제 host·agent에서 설정이 main을 채우고 모달 배경·aria-modal 없이 표시되는지 검사한다. Chromium 1280px 밝음/어두움·980px 좁은 본문·320px, WebKit 390px 밝음/어두움에서 상태·설명 폭과 가로 넘침을 확인하고 캡처한다. 10분이 지난 상태와 긴 도메인의 등록 상태는 실제 `/me` 응답의 passkey 필드만 대체해 기다림 없이 재현한다. 인증·등록 동작은 기존 E2E79가 맡는다.
- 주소 복사·새 탭·새로고침·닫기·앞으로, 확인 창의 Esc, 같은 pane을 사이드바에서 다시 선택하는 경로, 모양/터미널/단축키/알림의 긴 값도 확인한다. 테마는 실제 설정 조작으로 바꾸고 적용 사건을 기다린다.
- 기존 설정 region·키 바 region 접근성 locator를 갱신한다. 휴대폰에서 아래로 밀면 본문에 남고 X로 나오는 새 계약을 검사한다. 닫은 뒤 focus는 데스크톱 톱니·터미널 또는 휴대폰 본문 목록 단추여야 한다. 폰트 다운로드·격자/PTY 크기·키보드 입력과 브라우저 뒤로의 기존 단언은 유지한다.
- 초기 회귀는 닫기 focus의 한 프레임 지연을 찾았다. 보이는 활동 막대에는 즉시 돌리고 가려진 본문은 commit 뒤에 돌린다. WebKit은 서비스 워커를 거친 `/me` 요청이 시험 route를 우회해 실제 loopback 응답을 받았다. 응답을 대체하는 새 단계만 기존 시험과 같이 serviceWorkers=block으로 격리하고 받은 passkey 필드도 확인한다. 제품 인증·서비스 워커 동작은 바꾸지 않는다.
- 전체 E2E04의 wake lock과 E2E14의 공유 목록 모양 검사는 설정을 닫으면 목록 drawer가 그대로 열려 있다는 옛 모달 가정을 사용했다. 본문 복귀 뒤 drawer를 직접 열도록 동작을 갱신하고 기존 요청/해제 횟수·저장·새로고침·Reader 단언을 유지한다. 새 Tab 검사는 특정 톱니가 아니라 실제 DOM 차례의 앱 목록에 focus가 나왔는지 확인한다.
- 첫 전체 병렬 gate에서 load average 76.31을 관찰했고 Rust m13 6개 준비 사건과 E2E78 계정 기동·E2E82 후보 점검이 실패했다. 원본 로그를 보존했다. 제품/시한/단언을 바꾸지 않은 Rust 6개 순차 실행과 E2E78·82 단독 파일이 모두 통과했다. 후속 전체 gate는 `--tee`로 단계를 순차 실행하고 Rust worker 3·E2E worker 3으로 모든 검사를 유지했다. 웹 E2E 88파일/99작업이 통과했다. 이 실행의 `RUST_TEST_THREADS=1`은 기존 aaa-common panic_log 자식 출력에 libtest 접두어를 붙여 파서 시험 하나를 실패시켰다. 자식의 성공·출력 형식을 직접 확인하고 저장소 기본값 2로 aaa-common 36개를 따로 검증했다. 기본 test thread 설정은 유지한다. 새 UI 변경 뒤에는 web/build와 해당 E2E를 검증한다.
- E2E26의 긴 목록 단계(`web/e2e/lib/panel-scroll.mjs`)는 실제 격리 host에 답한 것 79개·할 일 79개·기록 79개·자동화 40개와 앞 단계의 보고서 14개를 준비한다. DB fixture는 host를 멈춘 동안만 쓴다. Chromium·WebKit 각각 1280×800·390×844에서 제한된 높이·가로 넘침·스크롤 끝·hit test·항목 선택을 확인한다. 데스크톱은 실제 휠의 scroll 사건을 기다린다(scrollTop 변화는 MutationObserver가 알리지 않는다). 휴대폰은 native overflow의 scrollTop으로 끝을 확인한다(합성 touchmove를 실제 손가락 관성 스크롤로 주장하지 않는다). 기록은 더 보기가 없어질 때까지 페이지를 읽고 마지막 기록의 전후 차이도 연다. 머리·바닥과 문서는 움직이지 않는다. 기존 코드에서는 질문 목록 높이 3,484px가 800px 화면 밖으로 늘어나 첫 단언이 실패했다.
- 착지 전체 Rust의 `post_cli`에서 3초 응답 시한·가짜 서버 BrokenPipe가 관측됐다. ACK/종료 코드 fixture가 실제 `hooks::read_msg`와 달리 완성된 JSON 뒤에도 EOF를 기다렸다. 요청 쓰기 쪽을 열어 둔 socket pair로 이 추가 의존성을 RED 재현하고, 첫 JSON 완성에 답하도록 고쳐 GREEN을 확인한다. 기존 옛 agent/accepted/refused/queued의 종료 코드·응답 단언과 제품 3초 시한은 유지한다. 제품 코드는 바꾸지 않으며 고정 sleep 없이 읽기 사건으로 끝낸다.
- 고정 sleep·시한 연장·실패 재시도 없이 DOM·응답·history 사건으로 끝낸다. `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 06-settings --workers 1`과 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행한다.

## 왼쪽 목록 고정 번호 칩(t-0739, plan §8.442)

- `nice -n 15 cargo test -p aaa-host --lib automation_ids --locked`: 실제 host·DB·파일로 기존 규칙 조회→제목 수정→형식 변환→삭제→다른 이름 번호 발급→복원→재시작·프로젝트 이름 변경·내보내기를 확인한다. DB의 동시 읽기와 서로 다른 이름 발급은 Barrier 사건으로 끝내며 프로젝트별 독립 순번·재열기·프로젝트 삭제를 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 21-library-reports,34-automations,43-tasks,69-coordinator-questions --workers 1`: 실제 host의 r/d/q/a 번호를 공유 칩으로 보이는지 검사한다. 보고서 inline code 기준의 글꼴·크기·배경·색·여백·모서리를 밝음/어두움에서 비교한다. Chromium 데스크톱·휴대폰·320px과 WebKit 질문 목록 390/320px에서 번호 전체·행 넘침을 확인하고 캡처한다. 긴 제목을 DOM에 잠깐 적용해 제목만 줄고 번호가 남는지도 검사한 뒤 원래 DOM을 복원한다.
- 질문 행은 공통 button의 고정 높이를 덮어 내용만큼 커지고 flex로 줄어들지 않아야 한다. 추가한 세로 넘침 검사는 수리 전 실행 파일에서 네 q-ID의 행 넘침으로 실패하고 수리 뒤 통과했다. E2E69는 질문 본문과 sidebar의 선택 완료를 모두 기다린 뒤 다음 pane으로 이동하며, 단계 실패 때도 별도 WebKit을 finally에서 닫는다.
- 여러 질문의 모든 ID·coordinator 첫 질문 중복 없음·답한 기록 ID 보존, 자동화 변환 뒤 같은 ID와 기존 이동·편집·답·Reader 거부도 유지한다. 고정 sleep·실패 재시도·시한 연장은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.107, Agent link 1.11이며 착지 때 main의 문서/API 번호와 다시 대조한다.
## 할 일 공통 검색줄·칩 분류·완료 체크(t-0737, plan §8.441)

- `node --test web/test/tasksModel.test.mjs`: 모델만 정한 대기와 담당·실행·예약·오류·완료 조합을 저장소별로 확인한다. 전체 외 칩의 합=전체, 각 칩의 실제 행 수=표시 수, 모든 ID의 중복/누락 없음을 검사한다. 예약 상위가 다른 칩에 있어도 하위는 자기 칩에 남는다.
- `nice -n 15 cargo test -p aaa-host --lib task_queue::tests:: --locked`: thread 없는 `[x]`/`[X]`를 하위부터 보관하고 미완료 하위가 남은 상위는 보존한다. 담당·메모·고정 ID·별도 완료 근거, 재시작 뒤 중복 없음, 복원 뒤 체크 해제·thread 없음·재예약 없음, 보관 저장 실패의 원본 보존과 파일 제거 뒤 확정 실패의 재시작 회수를 검사한다.
- `nice -n 15 cargo test -p aaa --test m13 thread_start_warns_about_similar_tasks_without_blocking --locked`: 완료 체크의 보관 tick을 확정한 뒤 활성 유사 task 경고·명시 연결·기동 비차단을 검사한다. 같은 제목의 완료 항목은 원래 ID로 보관되고 새 기동은 별도 ID와 열린 thread를 가져야 한다. 옛 시험의 완료 항목 활성 잔류 가정을 새 보관 계약으로 대체하며 경고·원본 머리·고정 ID 보존 검사는 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 86-task-history --workers 1`: 실제 host·agent에서 Chromium 1920px와 Chromium 320px·WebKit 390px Reader의 탭/검색 공통 부모, 검색 위치·높이·스타일·간격과 좁은 화면의 동일 줄바꿈을 비교한다. 기록·보관 두 입구, 기존 검색·저장소·상태·더 보기·복원/Reader 권한을 유지한다. 실험실 파일의 모델 지정 대기·칩 합·직접 완료 체크·보관/복원도 확인한다. 운영 TASKS.md는 수정하지 않는다.
- DOM·응답·`ticker tick`을 완료 사건으로 쓰며 고정 sleep·실패 재시도·시한 변경은 없다. 배치는 board의 실제 hash와 Front 기능/권한 응답 수신 뒤 잰다. 수신 전에는 「기록」 탭이 없어 검색 시작 위치가 달라지므로 첫 응답 전 폭을 기준값으로 삼지 않는다. 기존 E2E43·46·47·67·76·85와 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사를 수행하고 착지 창을 기다린다. API/E2E 번호는 유지한다.
- t-0753 착지 검증에서 E2E86의 완료 체크 복원 뒤 즉시 CLI 조회가 실패했다. 보관 목록은 복원 요청을 시작할 때도 비워지므로 행 없음은 완료 신호가 아니다. 해당 task ID의 실제 `task_archive` 복원 성공 응답과 `aria-busy=false`·빈 결과 문구를 확인한 뒤 기존 ID·체크 해제·연결 없음·owner·예약 없음 단언을 수행한다. 남겨진 실패 실험실에는 복원된 항목이 있었으며 제품 코드·시한·기대값은 바꾸지 않는다.



## 시스템 idle-audit의 웹 읽기 전용(t-0735, plan §8.440)

- `nice -n 15 cargo test -p aaa-host --lib automation:: --locked`: 실제 파일과 host 경계에서 Reader/Controller 원문·기록 조회, 웹 저장·토글·삭제·실행·승인·큐 변경·복원·옛 형식 변환의 거부와 파일 무변경을 확인한다. 파일 없음·오류에서도 이름 보호가 유지되고, 다른 운영 규칙은 웹 편집을 허용해야 한다. Local CLI 경계의 저장·토글·실행·삭제·꺼진 복원을 확인한다. 프로젝트를 생략한 일반 변환의 허용과 시스템 변환의 거부도 같은 경계에서 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 55-automation-edit --workers 1`: 기존 일반 규칙의 만들기·편집·삭제·복원·실행·승인·Reader 검사를 유지한다. 시스템 규칙은 Chromium 1280px·WebKit 390px에서 목록/상세 표지·이유·꺼진 스위치·쓰기 단추 없음·원문·기록·가로 넘침을 확인하고 캡처한다. 실제 웹 API의 각 변경 거부와 CLI 토글·삭제·복원도 확인한다.
- 완료 신호는 host 응답·파일·DOM이며 고정 sleep·실패 재시도·시한 변경은 없다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.106·AGENT_LINK 1.11·기존 E2E 번호를 유지한다.

## agent 연결 종료 뒤 보고 복사 대기 해제(t-0734, plan §8.439)

- `nice -n 15 cargo test -p aaa-host --lib link::supervision_tests --locked`: 실제 Agent link reader에 잘못된 frame을 보내 끝내되 peer와 writer는 살려 둔다. 종료 전 요청과 이전 연결 사본의 FileRead·ReportFileRead·Input은 즉시 AgentDisconnected가 되어야 한다. 재연결 뒤 옛 generation의 종료가 새 요청을 끊으면 안 된다. 수정 전에는 후속 FileRead가 Timeout으로 실패하고 수정 뒤에는 가상 시계가 전진하지 않는다.
- 같은 묶음에서 실제 512칸 전송 대기열을 채운다. enqueue에서 기다리는 요청도 연결 종료로 즉시 풀리고, 연결이 살아 있는 포화 대기열은 기존 15초 시한 안에 Timeout·pending 해제로 끝나야 한다. reader panic 정리·감독자의 실제 시한 초과 경고와 이후 성공 응답 시험도 유지한다.
- `nice -n 15 cargo test -p aaa-host --lib orch::report_tests --locked`: 실제 보고 수집 owner가 manifest 뒤 첫 파일에서 연결을 잃는다. 두 번째 파일이 이전 연결을 재사용해도 파일마다 120초 기다리지 않고, 새 본문·실패 상태와 이전 검증 첨부를 보존해야 한다. 새 연결을 넣은 뒤 hash 변화 없는 다음 ticker에서 complete·첨부 복구·ack 보존을 확인한다.
- 고정 sleep·실패 재시도·시한 연장은 없다. link의 상대 I/O와 보고 전송 응답만 대체하며 연결 수명·보고 수집·원장·파일 저장은 제품 owner를 쓴다. 전체 ci-local·공개 검사와 Linux 대상 clippy를 수행한다. 외부 API·웹 동작을 바꾸지 않아 기존 API/E2E 번호를 유지한다.
- 전체 검증 중 E2E69의 질문 링크 연속 누름에서 본문 render 직후의 사이드바 전환 effect와 다음 목록 누름이 경합했다. 정확한 q-ID·카드·본문 단언에 「답할 것」 활동 단추 선택과 패널 표시 완료 단언을 더했다. 다음 목록 누름은 이 DOM 사건 뒤에 시작하며 기존 시한과 나머지 단언을 유지한다.

## 맨 q-ID도 질문 상태 칩으로 표시(t-0730, plan §8.438)

- `node --test web/test/appIds.test.mjs`: 문장·괄호·붙은 `·`/쉼표·줄바꿈에서 두 q-ID를 각각 인식하고 주변 글자를 보존한다. 주소·경로·없는 ID 제외 검사는 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions,77-app-ids --workers 1`: 실제 host·agent·Claude 대화에서 Markdown 링크·옛 참조·맨 ID를 비교한다. E2E69는 thread·coordinator의 네 질문을 문장·붙은 괄호·쉼표 안에 넣고 모든 칩의 아이콘·본문·상태와 밝음/어두움 배경·아이콘 색 일치를 확인한다. 각 맨 ID를 눌러 정확한 q-ID와 카드로 이동하는지도 검사한다.
- 열림→답함→새 보고 뒤 답 보존은 새로고침 없이 확인한다. 답 기록까지 삭제해 host 목록에서 사라진 맨 ID는 평문, 명시 참조는 중립 칩이라는 기존 규칙을 유지한다. E2E77은 coordinator·thread 대화와 보고서의 질문 칩, 코드 블록·외부 링크 안의 칩 제외, 질문 이동·답 기록과 나머지 ID의 기존 링크/가운데 누름을 확인한다.
- 고정 sleep·실패 재시도·시한 변경 없이 DOM·주소·host 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사이며 착지 창 전 병합·push는 하지 않는다. API와 E2E 파일 번호는 바꾸지 않는다.

## 활동 막대 자동화·할 일 아이콘(t-0727, plan §8.437)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 26-activity-bar --workers 1`: 막대의 자동화·할 일 단추가 새 아이콘(`lucide-aaa-automation`·`lucide-aaa-tasks`)이고 옛 Workflow·ListTodo가 막대에 없는지 본다. 명령 팔레트 「할 일 열기」도 같은 아이콘인지 확인하고 막대를 캡처한다.
- `--spec 48-automation-threads`: thread ⋯ 메뉴 「자동화 규칙」이 막대와 같은 자동화 아이콘인지 확인한다. 둘 다 DOM 상태로 끝나며 고정 sleep·재시도는 없다. 그림 자체는 데스크톱·휴대폰 × 어두움·밝음 화면으로 눈으로 확인한다.

## 작업 목록 메뉴의 요약(t-0721, plan §8.434)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 host·agent에 요약 있는 thread와 없는 thread를 만든다. 작업 목록 점 세 개 메뉴의 「요약」으로 thread 머리 단추와 같은 상세가 열리는지 확인한다.
- Chromium 데스크톱·320px Reader와 WebKit 390px Reader에서 메뉴 본문 미표시·다른 thread의 정확한 목표·요약 없음 안내·hover 겹침 없음·주소 보존·가로 넘침을 검사한다. 데스크톱 키보드 Enter·Escape, 휴대폰 메뉴 시트→요약 시트 교체 뒤 한 번 뒤로 가기를 확인하고 각 화면을 캡처한다.
- 기존 E2E28의 머리 단추·hover·고정·지시 전문·실시간 진행·Reader 쓰기 거부 검사는 유지한다. 메뉴 해제·DOM·애니메이션·history 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.
- E2E12의 thread 메뉴 전체 항목 검사도 「보고·요약·고정·모든 thread」 순서를 검증한다. 기존 고정·닫기·다시 열기·모든 thread 이동 검사는 유지한다.

## host Codex 계정 이미지 생성(t-0707, plan §8.433)

- `nice -n 15 cargo test -p aaa-host --lib image --locked`: 이미지/일반 thread의 공유 예약,
  주간 잔량 51%의 4·8회 연속 요청·medium 공존·짧은 창 부족,
  남은 15%에서 두 동시 이미지 claim의 한 번 배정, 명시 별칭에서도 다른 공유 별칭의 관측 유지,
  미관측 첫 실행 중복 거부, 종료 뒤 새 관측에서만 해제, 재시작 불명 예약 보존·잔여 인증 회수,
  출력 파일의 범위·symlink·형식·중복 item을 검사한다.
- `nice -n 15 cargo test -p aaa-host --lib image_rpc::tests::installed_codex_accepts_isolated_image_config_without_authentication --locked -- --ignored --exact`:
  설치된 Codex 0.163.0을 빈 격리 홈에서 initialize·config/read·modelProvider/capabilities/read로만 확인한다.
  로그인·인증 파일·thread/start·turn/start·이미지 생성은 없다. 설치판이 없는 CI에서는 ignored로 남긴다.
- `nice -n 15 cargo test -p aaa --test m13 image:: --locked`:
  실제 CLI·worker PID·hook token·로컬 IPC·TLS 원격 agent·host 원장·가짜 Codex·가짜 OAuth 제공자를 쓴다.
  두 계정의 선택·중앙 갱신 한 번·64KiB를 넘는 결과 전송·동일 SHA·인증/임시 홈 정리·토큰 합계,
  같은 ID 재전송·기본 변경·host/pane 재시작·파일 정리 뒤에도 재생성 없음,
  다른 thread·위조 token·세션 없음·허용하지 않은 계정/profile·임의 도구 요청 거부와 임의 CLI 판의 생성 진행을 검사한다.
  오류에 잘못 넣은 가짜 인증의 가림, worker 자기 목록·coordinator 소속 프로젝트 목록/정리도 확인한다.
  turn/start 응답보다 먼저 온 이미지 사건도 유지한다. 원문·토큰 조각·인코딩은 로그·DB·원격 파일에 없어야 한다.
- `scripts/fixtures/image-codex.py`는 제품의 격리 홈·환경·argv를 확인하고 이미지 사건만 만든다.
  인증 선택·원장·입력 검증·공유 예약·권한 검사는 대체하지 않는다. 외부 실계정·API 과금은 쓰지 않는다.
- `image_rpc::tests::refresh_waiting_on_the_central_gate_cannot_deliver_a_new_login_to_an_old_job`은
  실제 중앙 갱신 잠금·loopback proxy·금고를 쓴다. owner가 gate를 복제해 기다리는 사건 뒤 같은 별칭으로
  다시 로그인해, 이전 판에 새 인증을 주는 Served 응답이 옛 이미지 작업으로 나가지 않는지 확인한다.
  응답 뒤 세대 검사 전에는 200(새 인증), 수정 뒤에는 401/빈 JSON이다. 제공자·실인증·고정 sleep은 쓰지 않는다.
- 결함 심기: 실제 선택 owner에서 이미지 예약 목록만 비우면 공유 예산 시험이 두 번째 배정 단언으로 실패한다.
  원본 바이트를 복원한 뒤 같은 시험이 통과해야 한다. 시험은 응답·파일·pane 종료 사건으로 끝나며 고정 sleep·재시도·시한 연장은 없다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. 웹 UI·Front 요청 변화가 없어
  FRONT_API 1.106·AGENT_LINK 1.11·웹 E2E 최대 번호 86(88파일)를 유지한다. Agent link의 imagegen.v1은 양쪽이 알렸을 때만 쓴다.

새 기능을 만드는 thread가 따르는 짧은 규칙이다. 근거는 참고 넷(herdr 0.9.5·herdr-projects 0.2.35·herdr-web-ui `aa624cb`·t3code `4b1024da`)과 이 저장소에서 실제로 난 간헐 실패다. 전략과 도구 목록은 [plan §13](plan.md#13-테스트-전략과-직접-만들-e2e-도구)에 있다.

## 대화 질문 칩의 q-ID 상태 색(t-0712, plan §8.432)

- `node --test web/test/asksModel.test.mjs`: 같은 thread의 새 q-ID는 답할 것, 지난 q-ID는 보존한 답으로 구별한다. coordinator 평문·구조화 질문의 열림·답함·기록 삭제 후 중립도 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions --workers 1`: 실제 host·agent·Claude 대화 기록에 Markdown q-ID 링크와 `aaa:question/q-ID`를 넣는다. thread·coordinator의 두 질문씩 열림→답함→새 보고에서 질문 비움 뒤 답 보존→기록 삭제 뒤 중립을 새로고침 없이 확인한다. class와 문구, 밝음·어두움의 실제 배경·아이콘 색을 CSS 토큰과 비교하고 화면을 남긴다.
- 기존 E2E53의 thread ID 링크·dock·보존본·지우기 단언을 유지한다. 고정 sleep·재시도·시한 연장 없이 host 응답·보고 수집·DOM 사건으로 끝낸다. 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사를 수행하고 착지 창을 기다린다.


## 할 일 기록 검색·저장소·완료 상태(t-0703, plan §8.431)

- `nice -n 15 cargo test -p aaa-host --lib task_queue::tests:: --locked`: 실제 DB/파일로 4MiB 초과 보관의 개수·바이트 페이지와 모든 ID의 빠짐/중복 없음, trim/대소문자·메모/표시 ID·저장소·실제 완료 근거 조합, 같은 ID 복원·연결/예약 없음, 2MiB 이력의 목록 비전송/내구 보존, 일치하지 않는 100건 뒤의 기록 검색·삭제 복원을 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 86-task-history --workers 1`: 격리 DB 130건/4MiB 초과에서 CLI의 전체 128건 출력, API 페이지의 frame 크기와 모든 ID, UI의 검색·저장소·완료 상태 조합/더 보기·완료/삭제 복원과 Reader 거부를 확인한다. Chromium 1280/320px·WebKit 390px의 활성/기록 화면과 필터 결과를 캡처하고 가로 넘침을 검사한다.
- 기존 E2E46의 삭제·차이·되살리기, E2E47의 완료 본문·결과·ID와 E2E67의 예약 실행·복원 이후 재실행 없음 검사를 유지한다. 완료 보관 입구가 기록 탭을 여므로 복원 뒤 활성 행은 할 일 탭에서 확인한다.
- 고정 sleep·실패 재시도·시한 연장 없이 실제 응답과 파일·DOM 사건으로 끝낸다. 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다. FRONT_API 1.106·Agent link 1.11.



## 일반 작업의 할 일 선행(t-0705, plan §8.430)

- `cargo test -p aaa --test m13 manual_threads_always_have_tasks_and_archive_the_result --locked`: 실제 host·agent와 가짜 harness를 사용한다. 배경 기동 응답 전에 할 일 연결·선택 모델/머신/저장소를 확인하고, Markdown 머리/체크 목록이 별도 task가 되지 않는지·원문 파일 바이트·완료 보관의 ID/보고를 검사한다. resolve 응답은 thread 종료만 보장하므로 `w.h.ticks(1)`로 그 뒤 시작한 ticker의 보관 판정을 확정한 뒤 조회한다. 원래 task ID·완료 근거·보고 경로·활성 목록에서 제거됨과 뒤 tick의 중복 보관 없음을 검사한다(t-0753, macOS CI의 resolve 직후 보관 조회 경합). 없는 Git ref의 준비 실패에도 연결된 할 일이 남고 `--link-task` 없는 기존 할 일 시작이 같은 ID를 쓰는지 확인한다. 보고 회수·명령 응답·ticker 완료가 끝 신호이며 고정 sleep·시한 변경·실패 재시도는 없다.
- host `account_pool::tests::waiting_direct_start_keeps_one_task_across_passes_and_request_replays`는 계정이 없는 실제 배정 대기 두 pass·같은 시작 ID 재전송·DB 재열기에서 task 하나/같은 ID·thread 0건을 검사한다. RPC 응답을 주지 않는 가짜 연결로 불필요한 재조회를 드러낸다. 기존 배정/취소 경합 시험의 대기·Cancelled·내구성 단언은 유지한다.
- 기존 m13 유사 task 경고 시험은 별도 task 생성과 기존 완료 task 보존, `coordinator_flow`는 연결 옵션 없는 자동 연결, `doctor_files_brings_the_coordinator_guide_to_existing_projects`는 새 프로젝트와 낡은 안내 갱신의 동일 문구를 검사한다. `automation_start_holds_approval_two_agents_and_restart_receipt`의 기존 실제 do.start 실행에서 할 일을 만들지 않는 예외를 검사한다.
- E2E47은 실제 일반 기동의 자동 연결을 확인한 뒤 그 할 일을 명시 삭제하여 옛 thread의 잇기/만들기 복구 단언을 유지한다. E2E48은 실제 승인된 do.start의 무생성·일반 작업 연결을 함께 확인한다. 전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사 뒤 착지 창을 기다린다.

## thread 보류·이어 하기(t-0689, plan §8.426)

- `nice -n 15 cargo test -p aaa-host --lib park:: restart_plan --locked`: 보류 대상(일반 Worker만)·닫기 판정(맡긴 뒤의 새 `[보류 준비됨]`이고 agent가 일·확인창을 기다리지 않음)·같은 대화 판정(대화 ID 자리와 UUID)·git 글, 보류는 `unpark`만 다시 띄운다.
- `nice -n 15 cargo test -p aaa --test m13 thread_park --locked`: 실제 host·agent·가짜 Claude(`resumable`)·TLS labB·로컬 origin. 고정 sleep은 auto-resolve 하루(시험 1초)를 넘기는 한 번뿐이고 그 밖은 host 기록·화면 출력·`ticker tick`으로 끝낸다. 결함 심기: brief 전달에서 `brief_line` 대신 brief.md 표지를 쓰면 「보류를 풀었다」 출력 기다림으로 실패한다.
- t-0720 인증 회귀: 같은 `thread_park` 묶음의 `account_unpark` 두 시험은 host 금고에 가짜 Claude 계정 둘을 등록하고 기본 계정으로 worktree를 시작한다. 보류 뒤 기본 계정을 바꿔도 같은 머신의 `--resume`·대화 ID·격리 홈 기록, TLS 다른 머신의 새 대화·같은 branch, 앞 머신 복귀의 원래 홈을 유지해야 한다. 계정 선택·실행 인증 원장·격리 홈은 제품 코드이고 harness만 대체한다. 기존 코드에서 두 시험이 모두 운영과 같은 `bad_request`로 실패하고 수리 뒤 통과하는지 확인한다. 운영 보류 thread·실제 로그인은 쓰지 않는다.
- `nice -n 15 cargo test -p aaa-host --lib account_policy --locked`: 보류 인증 참조 회수는 같은 실행·계정·harness이고 pane이 사라졌을 때만 허용한다. 기록 없음·다른 신원·준비 중·남은 pane 거부와 agent 응답 전 중복 기동 거부를 함께 검사한다. 고정 sleep·재시도·시한 연장은 없다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 85-thread-park --workers 1`: Chromium 1280px에서 「정리 맡기고 보류」·보류 띠·sidebar 제외·할 일 「보류」 칩과 거르기·「모든 thread」 보류 묶음·이어 하기 창, WebKit 390px에서 보류 창·바로 보류·이어 하기 창이 폭 안에 드는지 본다. 웹 유닛은 `operateModel`·`tasksModel`.
## 일반 post 기본 한도 확대(t-0688, plan §8.425)

- `nice -n 15 cargo test -p aaa-config --lib post_defaults_and_explicit_overrides --locked`: 파일 없음·빈 `[post]`·일부 생략에서 30/60초·200/시간·2000/일·pair 100·mute 20을 읽고, 명시한 옛 값과 local 덮어쓰기를 유지한다.
- `nice -n 15 cargo test -p aaa-host --lib post::tests:: --locked`: 실제 host DB와 전달 owner에 `now`를 주어 기본 30건 핑퐁 수락, 31번째 거부·19회까지 음소거 없음·20회 음소거, 분·시간·하루 경계와 24시간 내용 중복을 검사한다. 기존 ID 재전송·재시작 내구성과 작은 한도 설정도 유지한다. coordinator 사용자 알림은 별도 30/분·60/시간 한도와 자동 음소거 없음, 일반 한도 초과 뒤에도 수락을 확인한다.
- 기존 m13 `post_from_thread_reaches_coordinator_inbox`와 웹 E2E19 `post 음소거`는 전송·표시를 검사하도록 작은 한도(3/분·6/시간·30/일·거부 3회)를 명시한다. 제품 기본값에 기대지 않고 기존 단언·시한을 유지한다. 새 E2E 번호는 추가하지 않는다.
- 고정 sleep·실패 재시도·시한 연장 없이 원장 결과와 CLI 완료·DOM 사건으로 끝낸다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.
## 화면·고정폭·터미널 글꼴(t-0683, plan §8.423)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 06-settings --workers 1`:
  실제 host의 Chromium 1280px·WebKit 390px에서 세 글꼴을 바꾸고 새로고침 뒤 입력값·실제 CSS 목록을 대조한다.
  없는 글꼴만 고르면 기본 목록과 canvas 너비가 같아야 하며, 고정폭 목록은 아이콘 글꼴을 맨 앞에 둔다.
  화면·코드·터미널의 즉시 적용, 같은 기기 다른 탭의 storage 사건, 다른 기기 분리,
  세 「기본값으로」의 저장값 삭제·기본 목록 복원·새로고침 유지와 가로 넘침을 검사한다.
  설정의 모양·터미널 화면을 두 기기에서 캡처한다. 기존 face gate·설치 글꼴 거부·PTY 크기 검사는 유지한다.
  DOM·글꼴·storage 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다.



## 보고로 자동 갱신하는 작업 요약(t-0682, plan §8.422)

- `nice -n 15 cargo test -p aaa-host --lib progress --locked`: 실제 보고 수집 owner가 진행 목록·완료 수를 고치면서 목표·사용자 결정을 보존하는지 검사한다. 같은 보고 재수집·host 재시작 뒤에도 나중 지시 경고가 남아야 한다. 수집 시작과 수신 사이에 지시를 넣어 그 지시를 반영했다고 세지 않는 차례를 고정한다. 잘못된 목록·코드 예제·중복 절·항목/길이 상한은 마지막 정상 목록을 보존한다.
- 같은 host 시험은 실제 harness 사건으로 같은 활동의 진행률 갱신·정기 보고에서 활동 시작을 유지하고, 활동 변경·새 세션·대기 답변에서 초기화하는지 검사한다. 오래된 진행 사건도 새 활동 시계를 덮지 못한다.
- `node --test web/test/summaryCard.test.mjs`: 시작 후/활동 시간의 분리·진행률 미정·옛 host의 활동 시작 누락·끝난 thread 시간과 음수 경계를 확인한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 28-thread-summary --workers 1`: 실제 agent의 보고 감시→host 수집→열린 hover/상세 카드의 목록·완료 수·경고 갱신을 확인한다. pane 안 실제 CLI의 활동·진행률 갱신, 가상 시계의 경과 시간, coordinator 목표만 편집할 때 진행 보존, Chromium 320px·WebKit 390px Reader 표시·쓰기 거부를 함께 검사한다. 고정 sleep·실패 재시도·시한 연장은 쓰지 않는다.
- 결함 심기: 보고 수집 owner의 진행 전달 호출을 제거하면 실제 수집 시험이 진행 항목 `1 != 2`로 실패한다. 제품 원본 바이트를 복원한 뒤 전체 gate를 수행한다.
- 첫 전체 gate는 319초로 web·fmt·build·clippy·Rust 전체·public을 통과했고 E2E는 84/85 파일 통과다. 82 자동화 만들기의 저장 단추가 비활성 상태로 30초를 넘겼다(같은 실행 중 1분 부하 77.89). 저장 실패 때 주소·이름·입력값·비활성 상태를 남기는 진단만 더한 동일 제품의 단독 82 전체는 29초에 통과했다. 재시도·시한/기대값 변경으로 첫 실패를 지우지 않으며 원인은 미확정이다. 착지 통합 검증에서 원본과 진단을 함께 확인한다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다. FRONT_API는 1.104, Agent link는 1.11을 유지한다.
- 착지 기준 `8ddca9dc` 통합본은 전체 ci-local 217초에 일곱 단계 모두 통과했다. 웹 유닛 471개·Rust 전체·의존성 경계·웹 E2E 85개 파일(96개 작업)·공개 검사를 확인했다. E2E 28 전체와 82 전체(20.5초)가 통과했고 저장 단추 실패는 재발하지 않았다. 이 실행의 기록한 1분 부하는 10.69~86.22이며 시한 초과나 단독 재실행은 없었다. 최초 82 실패의 원인은 여전히 미확정으로 보존한다.


## PR routine 재변환과 보존 사본(t-0684, plan §8.421)

PR 기능 폐지(t-1203)로 해당 owner를 제거하거나 일정 규칙 fixture로 전환했다. 현재 회귀는 아래 t-1203 절을 따른다.

## install macOS launchd owner(t-0485, plan §8.419)

- `nice -n 15 cargo test -p aaa --bin aaa --locked cli::tests::launchd_registration_authorization_arguments_parse_before_any_service_changes -- --exact`: 실제 등록 owner와 공유한 승인 argv를 실제 `Cli::try_parse_from`으로 검사한다. `--prompt plain`은 install 상위 옵션이므로 authorize 앞에 둔다. 서비스 등록 없이 잘못된 순서 RED→올바른 순서 GREEN을 남긴다.

- `nice -n 15 cargo test -p aaa --bin aaa install::engine --locked`: 실제 임시 파일 receipt와 서명 승인,
  launcher package 적용 뒤 codesign, 보존 launcher의 승인 뒤 편집 무쓰기 거부,
  같은 ID 재개·공유 instance·외부 편집/가짜 ready 거부를 검사한다.
  OS manager만 대체하며 파일 transaction·ready 판정·journal은 실제 owner를 사용한다.
- `nice -n 15 cargo test -p aaa --test install --locked`: 읽기 launchd 계획과 package 누락의 무쓰기 경계를 포함한다.
  실제 등록 시험 하나는 로컬에서 ignored이며 운영 LaunchAgent를 변경하지 않는다.
- `launchd_authorization_and_apply_preserve_the_reviewed_service_environment_without_registering`은 실제 PTY 승인 뒤 apply와 같은 명령 환경의 dry-run 해시를 비교한다. TERM 누락의 RED와 같은 TERM의 GREEN, 다른 TERM의 해시 변경·무쓰기를 검사한다. 실제 등록은 실행하지 않는다. 같은 canonical state에서 host의 agent/front listener 두 개도 실제 bind/drop한다. macOS의 긴 기본 임시 경로가 UDS 한도를 넘는 RED 뒤, 기존 backend owner처럼 짧은 `/tmp` 실험실 root로 GREEN을 남긴다.
- 승인된 기존 macos-15 CI 단계는 `cargo test -p aaa --test install --locked launchd::launchd_real_registration_ready_resume_and_second_instance_preserve_the_shared_launcher -- --ignored --exact --nocapture`를 필수 실행한다.
  고유 두 label의 실제 등록·ready·bootout 뒤 같은 ID resume·두 번째 instance와 launcher 보존·정상 해제, 실패 시 자기 label만 정리한다.
  GUI 도메인이 없으면 실패이며 skip/fake-ready로 바꾸지 않는다. 다른 OS에서는 Mac native owner만 cfg 제외한다.
- release producer/manifest는 `nice -n 15 python3 -m unittest scripts.test_release`, 실제 app producer→consumer는 위 파일 owner가 맡는다.
  package 판/CPU/binary 결속 제거·외부 plist 덮어쓰기 변형은 owner 실패를 남겨야 한다.
- 서명 RED는 번들 안에 영구 자원 lock/reference가 생겨 codesign이 거부한 것이다. 메타데이터를 번들 바깥에 두어
  파일별 journal·공유 소유권을 보존하며 서명 GREEN을 확인한다. 전체 ci-local·deps·공개 검사·Windows cross 뒤 후보를 보고하고 착지 창을 기다린다.


## 계정 배정 대기 취소의 경합(t-0700, plan §8.428)

- `nice -n 15 cargo test -p aaa-host --lib account_pool --locked`:
  `cancel_waits_for_allocation_and_cannot_be_overwritten_or_restarted`는 실제 `pool_run`을 사용량 cache 잠금의
  await에서 보류하고, 배정 잠금을 잡은 사건 뒤 실제 Front 요청으로 취소한다. 수정 전의 즉시 BadRequest를
  재현하며, 수정 뒤에는 취소가 기다려야 한다. 조회를 풀어 실제 Queued 저장이 끝나면 Cancelled를 기록하고,
  다음 pass·같은 ID 재처리·DB 재열기 뒤에도 취소와 thread 0건을 유지한다. 운영 코드에 지연 고리는 넣지 않는다.
- `cancel_rechecks_a_receipt_committed_by_the_inflight_allocation`은 기다리는 사이 배정 영수증이 생기면
  기존 「이미 시작한 thread는 thread에서 끝낸다」 conflict를 보존하는지 검사한다. Reader의 잠금 대기 전 쓰기 거부,
  없는 요청의 NotFound·중복 취소의 같은 결과도 확인한다. 고정 sleep·재시도·시한 변경은 없다.
- 기존 웹 E2E81의 취소 click·창 닫힘·Cancelled·다음 tick에서 추가 기동 없음 단언과 30초 시한을 유지한다.
  실패하면 창의 오류·대기 문구·시작 상태와 host의 배정 요청 목록을 로그에 남긴다.
  전체 `nice -n 15 bash scripts/ci-local.sh`에서 다른 spec과 병렬로 검사한다.

- 착지 때 Linux 대상 `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --locked --keep-going -- -D warnings`도 확인한다. main install 엔진의 `File` import는 실제 사용 범위인 Windows·macOS·시험 cfg에만 둔다. Linux 제품의 미사용 import 실패를 경고 억제 없이 수리하며 Linux 실행 검증과 교차 lint는 구분한다.

- Windows CI 정리 실패 수리(t-0709): 가짜 머신의 `agent_connected`가 예약한 `rewatch`는 `Arc<Host>`와
  SQLite 연결을 보유한다. 두 연결 시험은 전용 current-thread runtime을 Lab보다 먼저 종료해 예약 작업을
  해제하고, Lab 정리는 `Weak<Host>`로 실제 Host 종료를 확인한 뒤 `remove_dir_all`을 한 번만 수행한다.
  기존 `tokio::test` 수명에 종료 단언만 더하면 macOS에서도 취소 경합 시험이 RED다. 취소 순서·Queued와
  Cancelled·다음 pass·재전송·DB 재열기·thread 0건과 할 일 한 건의 단언은 유지한다.
  원본 시험의 추가 진단에서도 재열기 Host의 명시 drop과 Weak 소멸 뒤 최초 Host의 strong count는 2로
  남아 정리 단언이 실패한다. 재열기 연결이 남았다는 가설과 예약 작업의 수명 문제를 구분한다.
  고정 sleep·정리 재시도·오류 무시는 없으며, Windows 교차 build/clippy와 실제 Windows CI 결과를 구분한다.

## host 공통 계정 풀 자동 배정(t-0653, plan §8.416)

- `nice -n 15 cargo test -p aaa-host --lib account_pool --locked`: 창별 최소 여유·모델 창·동점·공유 한도,
  네 동시 transaction의 예약 두 건·영수증 재생, 미관측 첫 실행·실패/오래됨/reset/NaN/누락 제외,
  확정 준비 실패 해제와 spawn 결과 불명 보존, 사람 권한·설정판 충돌·삭제 시 풀 제외를 검사한다.
  다른 프로젝트의 과거 DB 기록도 공유 한도 셈에 포함하며, 현재 host의 단일 프로젝트 제한은 유지한다.
  그룹 변경 뒤 기존 예약, 종료 전/후 관측·다시 로그인 뒤 예약 교체와 프로젝트 이름 변경/삭제의
  참여·대기 이동 및 전역 풀·관측 보존도 검사한다. `failure_after_waiting_reaches_the_inbox_once`는 대기 뒤
  pass에서 시작하지 못한 일이 inbox에 한 번만 오는지 본다(t-1198). 계정 화면의 시작 기록을 뺀 뒤 이 실패가
  웹 어디에도 보이지 않는 회귀를 막는다.
- `nice -n 15 cargo test -p aaa-host --lib quota --locked`: 공식 RPC의 실제 가짜 자식에서 무작업 조회,
  누락과 잘못된 숫자 구분·인증 세대·backoff·임시 인증 정리와 A2의 창별 관측 계약을 유지한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 81-account-pool --workers 1`:
  실제 host·agent·session·PTY에 가짜 Claude 상태줄을 연결한다. 웹 풀 편집·프로젝트 참여·신규 계정
  미편입, 기존 기본/명시 우선, 실제 관측의 자동 선택·같은 영수증, 미관측 첫 실행 뒤 대기 한 건,
  host 재시작 뒤 관측·대기 보존과 새 관측 뒤 기동, 계정 화면에 자동 배정 설명·시작 기록 칸이 없고 할 일 화면
  「계정 배정 대기」가 대기 중에만 보임(t-1198), 웹 할 일의 배정 대기·취소, WebKit 390px·Reader
  쓰기 거부·인증 비노출을 검사한다. 입력·상태줄 파일·host 응답·DOM 사건으로 끝낸다.
- 체크박스 E2E의 원본 실패는 비동기 저장 중 controlled checkbox가 즉시 옛 값으로 되돌아가는
  제품 결함이었다. 저장 중 선택값을 유지하고 실패 때 원래 값으로 돌아가게 고치며 같은 check 단언을 유지한다.
- 통합 gate의 CLI `large_enum_variant`는 시작 인자를 `Args` 구조체와 `Box`로 분리해 고친다.
  rollout의 연속 host 재시작 실패는 정상 연결이 10초 미만이면 이전 지수 backoff가 남아 원격
  agent가 30~45초 기다리는 것이 원인이었다. `aaa-agent`의
  `authenticated_short_connections_reset_backoff_but_rejections_preserve_it`는 실제 Welcome/replay
  사건과 Reject로 성공 뒤 250ms 초기화·인증 거부 뒤 기존 30초 보존을 확인한다. 초기화 제거
  변형은 `30s != 250ms`로 실패해야 한다. 기존 m13 `rollout_shared_host_agent_recovery_and_inflight_cancel_preserve_baselines`의
  30초 상한·두 agent 준비·복구/PID/파일 단언은 변경하지 않는다. main의 원격 fixture 재연결
  상한 1초도 보존한다. 이 시험 설정과 제품의 성공 사건 초기화는 별개이며 keeper는 상한과
  무관한 초기 30초를 사용한다.
- 고정 sleep·실패 재시도·시한 증가 없이 검사한다. 전체 필수 검증은 정본 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.

## Windows 짧은 state 경로와 IPC 완료 수명(t-0485, plan §8.418)

- 기존 Windows IPC owner의 모든 주소/반복·DACL·peer PID·byte·동시 첫 instance 거부·동일 주소 단일 rebind를 유지한다. GetShortPathNameW가 돌려준 실제 부모/canonical 양방향도 검사한다. 8.3 이름이 없는 파일시스템에서는 API가 같은 긴 이름을 돌려주며, 해당 환경에서 8.3 차이를 검증했다고 부르지 않는다.
- EOF 뒤 Rust 값 drop은 IOCP 합성 readiness가 소유한 Arc 해제와 다르다. 연결을 소유한 전용 runtime을 종료해 completion drain을 끝낸 뒤 새 runtime에서 같은 이름으로 한 번 rebind한다. 재시도·sleep 없이 실제 첫 instance 획득을 단언한다. 기존 연결/EOF 3초와 BUSY250ms·승인60초는 유지한다.
- 3933e4cb 실제 Windows RED를 보존한다. 로컬 macOS install·전체 ci-local·Windows cross clippy/build·check-public을 확인하고, Windows 실제 통과는 승인 착지 뒤 main CI만 판정한다.

## Windows install pairing·IPC 해제 owner(t-0485, plan §8.415)

- 원문 c249e316 Windows run의 pairing 조기 종료와 IPC bind OS5를 보존한다. PTY 실패 끝은 answered/전체·needle·child_exit, IPC 실패는 조합/반복·경로·OS 오류/해제 단계로 구분한다. 기존 승인/수명/비밀 가림 단언과60초를 유지한다.
- IPC의 기존3주소 조합×3연결을 유지하고 mixed/canonical 양방향을 더한다. 각 연결에서 byte 일치 후 client를 닫고 server EOF를 관측한 뒤 server를 닫는다. listener를 닫은 뒤 같은 주소 첫 instance를 단 한 번 재bind한다. drop만으로 handle 해제를 추정하지 않는다. 기존 연결3초·BUSY250ms를 유지하고 EOF도3초 사건 한도 안에서 기다린다. 재시도/sleep/시험 제외/기대값 완화는 없다.
- Windows 실제 통과는 main CI에서 확인한다. 그 전 cross는 컴파일/lint, macOS는 해당 OS의 회귀 검증으로 구분한다. rollout 수리는 t-0654 소관을 유지한다.

## 보고서 목록 첫 화면(t-0656, plan §8.414)

- `cargo test -p aaa-host --lib library_links --locked`: 모아 읽은 목록과 기존 단일 보고 조회를 비교한다. 같은 이름의 두 판·r 첨부 보관·끝난/진행 중 thread·원격에만 있는 파일·실시간 변경/원본 소실·안 봄 해제·링크 폐기 후에도 크기·보호·판·공유 표시가 같아야 한다. 기존 삭제·보존·링크 경계도 유지한다.
- `cargo test -p aaa-host --lib report_ids --locked`: 예약/완료/삭제 판의 분리와 보관 첨부의 원본 보호를 확인한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 80-report-list,21-library-reports,73-report-ids --workers 1`: 실제 host의 격리 DB·manifest에 1,118개 사본 폴더·559개 원본·최대 954개 첨부·보관 120건을 넣는다. 원문 바이트는 만들지 않으며 운영 library는 읽거나 수정하지 않는다. 요청 시작부터 JSON 수신까지와 두 animation frame 후 첫 화면 시간을 `report-list-timing.json`으로 남긴다. 시간은 관측값이며 부하에 민감한 통과 시한으로 쓰지 않는다.
- 첫 50개·추가 50개·끝까지 보기·화면 밖을 포함한 전체 선택, 보존 판을 별도 줄로 세지 않음을 확인한다. Chromium 1280px·WebKit 390px Reader의 실제 용량 응답을 route gate로 보류하고 목록이 보이는 사건을 먼저 기다린다. 용량 실패 때도 목록을 유지하며 가로 넘침·Reader 조작 부재를 검사한다. 고정 sleep·재시도·기대값 완화는 없다.
- 통합 검증의 `turn_end_notices_follow_reports`는 현재 brief 턴의 Working을 읽은 사건 커서 뒤에서 Finished를 고른다. 원본 실패에서는 앞 trust 턴의 Finished(현재 Working보다 1.2초 이전)를 처음 기록부터 다시 골랐다. kind·thread·짧은 턴·발생 시각·그 뒤 보고/일반 턴의 단언과 시한은 유지하며 실제 host·agent로 확인한다.
- 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사로 수행하고 착지 창을 기다린다. 운영 host 접근이 없는 실험실 결과를 운영 37GB 실측으로 부르지 않는다.


## 직접 병합한 thread의 할 일 완료 보관(t-0651, plan §8.412)

- `cargo test -p aaa-host --lib task_clean --locked`: 기존 PR·표지·중단·하위 task 판정과 명시한 병합 커밋 줄 파서를 검사한다. 일반 검증 SHA·코드 예제는 병합 근거로 읽지 않는다.
- `cargo test -p aaa --test m13 task_main_merge --locked`: 실제 host·agent·로컬 bare origin에서 `[착지]` branch 병합과 보고의 축약 병합 커밋으로 완료 보관한다. 오래된 origin/main·push 없는 로컬 main 병합·미병합 branch·보고 없음·사본 버림을 구분한다. 종료 응답 직후 attention 없음·active task 제거·archive 근거·coordinator inbox 한 건·중복 판정 없음과 로컬 ref/FETCH_HEAD 보존을 검사한다.
- `node web/e2e/web.e2e.mjs --spec 47-task-thread --workers 1`: 실제 Git 저장소의 직접 병합·미병합 thread를 끝내고, 병합된 할 일의 완료 보관과 「답할 것」 카드 부재, 미병합 할 일의 확인 카드 유지를 DOM 사건으로 확인한다. 기존 완료 복원·보고 없음·하위 task·시작 권한 검사를 유지한다.
- 고정 sleep·실패 재시도·시한 연장 없이 보고 회수·ticker·DOM 사건으로 끝낸다. 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다.



## install Windows Job·부모 종료·state IPC(t-0485, plan §8.411)

- main23328f7e Windows install의 원문6실패를 보존한다. 네 worker 실패는 OS5·Job0x2000이며 생산의 제한 거부는 맞는 동작이다. install 시험 실행기는 자기 nested Job만 breakaway 허용으로 만들고, worker 수명 owner는 별도 허용/금지 Job을 모두 검증한다. 바깥 runner 제한은 변경하지 않는다.
- 부모 종료는 소유 Job을 실제 TerminateJobObject(code1)로 끝내고 닫는다. 기존 부모 비정상 종료·worker 생존·worker 자신의 exit3 내구 기록·같은 ID 재개/읽기 status·금지 시 무쓰기 단언을 유지한다. timeout은 늘리지 않는다.
- 기존 Windows IPC ACL/peer/byte owner는 동일 경로3회에 일반/canonical 양방향3회씩을 더한다. 기존 주소를 먼저 열고 NotFound만 같은 실제 디렉터리의 별칭을 확인한다. 기존 BUSY·없는 pipe·첫 instance 보호도 계속 검사한다. Windows 실제 GREEN은 main CI만 인정한다.

## 계정별 Codex 대화 기록(t-0634, plan §8.407)

- `cargo test -p aaa-agent --lib chat_paths_tests --locked`: 저장된 CODEX_HOME·계정 홈 우선순위와 hook/rollout ID 불일치의 실제 JSONL을 검사한다. 같은 파일의 301줄 페이지와 ChatChanged 사건까지 확인하며, 중복 후보·다른 작업 폴더·하위 agent·이전 실행 기록·공유 홈 추측을 거부한다. 원래 경로 해석으로 되돌리면 세 keeper가 실패한다.
- `cargo test -p aaa --test m13 accounts::account_credentials_parallel_refresh_and_all_records_are_redacted --locked -- --exact`: 기존 로컬/원격 TLS 네 작업의 인증 회전·기록·재개·비노출을 유지하면서 서로 다른 hook/rollout ID를 넣는다. 실제 인증·운영 설정·모델 작업은 쓰지 않는다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 78-account-chat --workers 1`: 서로 다른 계정의 thread별 홈에 같은 ID 기록을 만들고 Chromium 1280px·WebKit 390px에서 조회 실패 중 Codex 머리말, 실제 PTY F6 입력 뒤 JSONL 생성과 자동 갱신, 이전 301줄·새 글·계정 분리를 확인한다. DOM·PTY 출력·기록 감시 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다.
- `node --test web/test/session-cleanup.test.mjs`: 실제 세션 트리의 PID 소유 확인·자손 종료와 다른 트리 보존과 배포 감시 후보의 명령줄 대조를 검사한다. 전체 프로세스 표는 PID·부모·상태만 읽어 다른 작업의 긴 명령줄에 출력 한도가 소진되지 않게 한다.
- 통합 E2E77의 삭제·종료 후 대화 복귀는 원문 표시와 링크 없음을 함께 기다린다. 비어 있는 대화에서 부정 조건만 먼저 통과하던 원본 실패를 보존하고, 같은 원문 단언·시한·제품 동작을 유지한다.
- 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 저장소 검사다.



## Git 자동화의 자기 착지 제외(t-0633, plan §8.406)

- `nice -n 15 cargo test -p aaa-host --lib git_watch --locked`: 격리 host DB와 GitQuery frame으로 실제
  git_repo·내구 outbox 전달을 끝낸다. 자기 규칙의 병합 여러 개와 내부 일반 커밋만 있으면 입력 0건·기록과
  routine list의 `자기 착지`, 사람 병합 혼합·다른 규칙 병합·두 규칙 혼합이면 입력 1건을 확인한다.
  승인한 hash·승인 상태 보존과 다음 같은 head 조회의 중복 입력·기록 없음도 확인한다.
- 병합 없음·없는 thread·번호 없는 병합·잘린 커밋 목록은 거르지 않는다. forced/deleted·다른 동작과 다른
  프로젝트의 출처도 거르지 않는 경계를 검사한다. 80자로 잘린 표시용 제목은 원문의 owners로 판정한다. 고정 sleep·재시도·시한 연장 없이 조회 응답으로 끝낸다.
- 자기 착지 판정을 제거한 변형은 자기 병합만 있는 경우의 큐 단언에서 실패해야 한다. 제품 바이트 복구 뒤
  전체 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사를 실행한다.

## 할 일 깊은 주소와 ID 자동 링크(t-0624, plan §8.404)

- t-0697: main CI run `39693201281`·`39699498898`의 원문과 artifact `e2e.log` 모두 깊은 주소 이동 전 `getByRole('menuitemcheckbox', { name: /^작업 폴더/ })`에서 30초를 넘겼다. 목록 도착 전 연 저장소 메뉴가 빈 항목을 복사해 둔 제품 경합이다. E2E77은 새로고침 전 실제 host의 `tasks` 응답을 gate로 보류하고, 「전체」만 있는 메뉴를 연 뒤 응답을 풀어 행이 나타나는 사건을 기다린다. 메뉴를 다시 열지 않고 「작업 폴더」를 선택하고 거르기 적용을 확인한다. 수정 전 같은 locator의 30초 실패를 재현했으며, 제품은 열린 저장소 메뉴의 항목·개수·선택 함수를 최신 board에서 만든다. 기존 깊은 주소·새로고침·다른 프로젝트·없는 번호·WebKit·Reader 단언과 시한은 유지한다. 고정 sleep·재시도·시한 연장은 없다.
- `node --test web/test/appIds.test.mjs`: 네 종류 ID·조사·문장 끝·5자리 번호·없는 번호, 주소·경로·낱말 경계와 프로젝트 있는/없는 할 일 주소를 확인한다.
- `node web/e2e/web.e2e.mjs --spec 77-app-ids --workers 1`: 실제 host에서 thread·할 일·coordinator 질문·보고서를 만들고 대화·보고서 본문의 네 ID를 누른다. 직접 주소·프로젝트 주소·새로고침·보조 키 새 탭·뒤로, 코드 블록/주소/없는 번호 제외, 답한 질문·삭제/복원·완료 보관을 확인한다. WebKit 390px·Reader도 첫 주소·상세 닫기·보고서에서 할 일 이동·가로 넘침을 검사한다.
- `cargo test -p aaa --test m13 doctor_files_brings_the_coordinator_guide_to_existing_projects --locked -- --exact`는 새 프로젝트와 `doctor files --fix`로 고친 기존 프로젝트의 ID만 쓰기·답할 질문 링크 규칙을 함께 검사한다. host의 `brief_text::tests::brief_order_and_memory_cap`도 새 brief의 규칙을 확인한다.
- 처음 구현의 thread 링크는 짧은 선택을 직접 쓰는 경로에서 hashchange와 정규화가 경합해 빈 화면에 머물렀다. E2E77의 네 종류 이동 단언이 이를 재현했다. 기존 thread 선택 함수로 내부 project/id를 고르고 같은 단언을 유지한다. Reader는 편집 폼 전용 표지 대신 실제 상세의 ID 글자를 대조한다.
- `d-9999`라는 이름으로 보고서를 발행해도 없는 할 일이 링크가 되면 안 된다. E2E77은 이 이름 충돌을 실제 publish로 만들고, 보고서의 실제 r 번호와 할 일 ID를 구분하는지 검사한다.
- 끝난 thread의 답하지 않은 질문은 보존 카드가 없으면 일반 글로 돌아간다. 기존 후보 binary에 새 E2E77을 실행해 해당 단언 RED를 보존했고, 열린 카드·답한 기록으로 존재 여부를 정한 후보에서 같은 단언 GREEN을 확인했다. 답한 질문 링크는 보존 기록으로 계속 열린다.
- 첫 전체 검증은 머신 부하 200~250에서 E2E72의 실제 SSH 감지가 시간 초과했다(host DB의 실패도 SSH 시간 초과). 부하가 내린 뒤 SSH 코드·시한·단언을 바꾸지 않은 단독 E2E72는 통과했다. 원본 실패를 보존하고 최종 전체 검증은 지원하는 worker 설정으로 동시 실행 수만 줄이며 모든 시험을 유지한다.
- 중간 검증에서 설치 승인 시험 실행(21:03:04~45)과 같은 target의 실행 파일 재빌드(21:03:10~27)가 겹쳤고, 해시를 묶는 승인 발급 두 시험이 종료 코드 4로 실패했다. binary를 고정한 두 시험과 후속 전체 설치 24개는 통과했다. 전체 gate가 도는 동안 같은 target의 aaa를 다시 빌드하지 않는다. 설치 제품·시험 시한·단언은 바꾸지 않는다.
- 고정 sleep·실패 재시도·상한 연장 없이 DOM·host 사건으로 끝낸다. 전체 정본은 `nice -n 15 bash scripts/ci-local.sh`다.

## install 종료 통지의 reap 독립성과 Windows 실패 진단(t-0485, plan §8.403)

- `authorization::terminal_exit::owner_reaping_after_subscription_preserves_the_child_exit_event`는 실제 shell 자식에 종료 구독을 준비하고, 실제 kill/wait로 먼저 reap한 뒤 같은 종료 통지가 와야 한다. 기존 waitid 변형은 errno10/ECHILD로 RED이고 구독 고정 후 GREEN이다. sleep·가짜 자식·production 허용 seam을 쓰지 않는다. Unix 전용인 까닭은 소유 reap과 waitid의 경합이며, Windows는 기존 실제 handle/ConPTY owner를 계속 돈다.
- `tests/support/child_exit.rs`의 구독은 driver callback 전에 고정한다. macOS kqueue/Linux pidfd는 reap하지 않으며, 이미 끝난 자식의 상태는 실제 소유 Child가 수집한다. Windows는 실제 process handle을 복제한다.
- Windows `input::password` console owner도 같은 구독을 쓴다. 실제 성공/취소/EOF·암호 일치/echo 없음/mode 복구·최종 출력을 유지하며, 기존20초를 늘리거나 사례를 빼지 않는다. install driver의 기존60초와 마지막300줄·종료7 owner도 유지한다.
- Windows 실패 단언은 redacted JSON·stderr·부모 종료 출력을 포함한다. worker spawn 실패의 OS 오류/Job 제한 비트와 human needs_user 요구가 CI 원인 판정 근거다. 기존 기대값/승인·private 표시·Job breakaway 정책은 유지한다. Windows 교차 clippy/build는 compile/lint 판정이며 실제 성공은 main CI로 구분한다.

## 원격 보고의 해시와 첨부 실패 격리(t-0638, plan §8.401)

- 후속 t-0654(plan §8.413)의 `orch::report_tests`는 `.claude`·`.reports` 하위와 경로 이탈을 거부하면서 최신 본문·불변 판이 도착하는지 검사한다. 판 저장 경로를 일반 파일로 막아도 본문·단계·정상 첨부를 갱신하고, 같은 hash의 ticker 복구가 ack를 보존하는지 확인한다. 수정 전 두 keeper는 각각 경로 거부·본문 미갱신으로 RED였다.
- `report_ids::tests::snapshot_`은 보존 판의 숨은 성분·symlink·크기 불일치를 계속 거부하고 깊은 일반 경로는 허용하는지 검사한다. t-0490의 크기 오류는 manifest 작성 후 바뀐 host 첨부에서 재현하며 크기 기대값을 완화하지 않는다.
- `cargo test -p aaa --test report_copy --locked`의 판 보존 사례는 실제 host·CLI·Agent link와 격리 SQLite의 판 공개 실패 trigger를 쓴다. 새 본문·경고 한 번·작업 상태·실패한 r 비공개, trigger 제거 후 새 hash나 변경 사건 없이 ticker로 같은 본문을 불변 판에 보존하는지 검사한다. 운영 DB·파일은 바꾸지 않는다.
- 판 복구는 보고 목록을 갱신하되 같은 본문·첨부의 sandbox 경고를 반복하지 않는다. 첨부 변경과 판 생성 신호를 합친 초기 후보는 실제 경고 2건으로 RED였고 신호 분리 뒤 같은 시험으로 확인한다.
- 후속 전체 검증에서 rollout의 마지막 host 재시작은 로컬 agent만 연결되고 원격은 준비 30초를 넘겨 실패했다. 원격 fixture의 `node()` 재생성이 기본 재연결 상한 60초를 적용했고, 10초보다 짧은 성공 연결·종료가 반복되며 16초 다음 32초 backoff로 커졌다. rollout fixture만 로컬과 같은 재연결 상한 1초를 명시한다. 제품의 backoff·요청/준비 시한·기존 rollout 단언은 유지하며 실패 로그를 보존한다.
- link의 사본 시험은 가짜 agent의 취소 완료와 host 연결의 정상 종료를 기다린 뒤 host를 놓고
  임시 폴더를 지운다(t-0647). 취소 요청만 하고 지우면 Windows에서 열린 DB 핸들 때문에
  정리가 os error 32로 실패할 수 있다. 재시도·sleep·정리 오류 무시 없이 기존 삭제 단언을 유지한다.
  Windows 실행 판정은 main CI의 해당 시험 결과로 하며 macOS 통과·교차 빌드와 구분한다.
- `cargo test -p aaa-host --lib report_ --locked`의 실제 Agent link keeper는 등록한 가짜 인증을 포함한
  원본 전체의 SHA와 가림 뒤 SHA가 다름을 재현한다. 원본을 변경하지 않은 채 512KiB 경계와 마지막 조각을
  전송해 먼저 검증하고, host의 현재 사본·불변 r 판에는 가린 바이트만 저장하는지 확인한다.
  원본 판 hash와 저장 본문 hash를 나누어 host 재시작의 단계 복구도 유지한다. 일반 파일 보기 가림 검사는 그대로다.
- `orch::report_tests`는 append prefix·rewrite 거부·이전 검증 파일 보존·저장 오류의 기존 검사를 유지하며,
  첨부 hash가 달라도 최신 보고 본문과 ack가 유지되고 실패 파일을 불변 판에 넣지 않는지 확인한다.
  같은 원본 판 재수집은 본문·저장 hash를 복구하면서 ack를 보존한다. 한 thread의 수집 lock을 잡은 상태에서
  다른 thread의 실제 수집이 끝나야 한다.
- `cargo test -p aaa --test report_copy --locked`는 실제 host·CLI·가짜 Agent link에서 manifest 실패와
  파일 하나의 rewrite 실패를 구별한다. rewrite 실패 중에도 새 보고·정상 첨부가 도착하고, 새 보고 hash나
  변경 사건 없이 ticker가 실패 첨부를 복구해야 한다. 경고 한 번·작업 상태 보존·기존 시한은 유지한다.
- `cargo test -p aaa --test m13 remote_report_with_registered_auth_bytes_copies_without_false_hash_failure --locked`는
  실제 TCP+TLS agent에 profile 인증 env를 등록하고, 바뀌지 않은 328KiB 로그와 보고를 회수한다.
  원본 보존·원본 판 hash·가린 최신/불변 사본·host 재시작 뒤 병합 대기 단계를 확인한다.
- 고정 sleep·시험 재시도·시한 증가 없이 수집 완료·보고 hash·연결 사건으로 끝낸다. 전체 검증은
  `nice -n 15 bash scripts/ci-local.sh`이며, 착지 때 main의 절 번호를 다시 맞춘다.



## 질문 카드의 강조·번호 목록·누락 경고(t-0618)

- `cargo test -p aaa-proto --lib --locked`: `m3-questions.md`는 host가 보존한 t-0578 답변 당시 Questions 원문이다.
  `1. **M3 순서**: …`, `- **A(추천)** …`, `2. **M3 끝까지 시험할 곳**`의 질문 2개·선택지 3/2개·추천과
  강조/간격 변형을 확인한다. 초기 구문 재현 fixture는 수정 전 첫 선택지 누락에서 RED였고 원문으로 교체했다.
- `t0621-questions.md`는 t-0621 Questions 원문이다. A~D와 각 들여쓴 그림 뒤에 E·F 설명·그림이 있어도
  질문 2개·선택지 4/3개·D 그림과 뒤 설명을 분리해 보존한다. 수정 전 같은 원문에서 첫 질문이 빠져 1개/3개로 RED였다.
  이때 누락 진단도 함께 발생한다. 원문에 없는 E/F 선택지를 만들지 않으며 줄을 깨뜨린 변형도 경고해야 한다.
  수정 전 제품 파서의 제어 대조에서 표를 네 행으로 줄이거나 들여쓴 그림을 지워도 누락됐다. 뒤 E/F 문단·그림만 지우면
  여섯 행 표와 들여쓴 그림을 둔 채 질문이 읽혔다. 실제 A~F 여섯 선택지도 읽혔다. 원인은 네 개 상한이 아니라
  뒤의 열 0 문단·그림에서 역방향 목록 탐색이 멈춘 것이다. 실제 상한은 9개이며 2~4개는 안내의 권장 형식이다.
- 파서 진단은 code fence 밖 A~Z 후보 줄 수·카드 수, 질문 수·선언 수와 번호/글자 순서를 비교한다.
  빠진 A·중복 A·A/C·인식 못한 강조·질문/선택지 상한·선택지 하나뿐인 질문은 경고다.
  code fence의 선택지 예시는 세지 않는다. `###` 본문의 일반 번호 목록을 보존하고 선택지 뒤의 `N.`만 다음 질문으로 읽는다.
  선택지가 하나뿐이거나 전부 읽히지 않아도 질문 제목과 경고를 남긴다. 9개 상한 초과도 제목과 경고를 유지한다.
- `cargo test -p aaa-host --lib question --locked`: 실제 보고 회수와 DB 경계에서 복사 성공·경고 유지,
  thread 전체 답과 q-ID 일부 답 거부(초안도 없음), 같은 보고 중복 경고 방지를 확인한다.
  이전 host가 다르게 읽은 미답 카드도 재기동 때 안전하게 잠그며 명시한 Questions를 기본 검수로 덮지 않는다.
- `node web/e2e/web.e2e.mjs --spec 53-thread-questions --workers 1`: 실제 host·agent·Chromium·WebKit에서
  M3 2개 질문·3/2개 선택지·추천, 누락 경고·전체 누락 카드·웹/CLI 거부, thread inbox JSON과
  coordinator 경고, 수정 뒤 `1C 2B`와 원래 선택지 설명 전달을 검사한다. 같은 파일의 기존 그림·답 보존 검사도 유지한다.
  같은 파일의 t-0621 원문은 글·표를 그대로 쓰고 그림 bytes만 실험실 PNG로 대체한다. A~D 그림, 뒤 E/F 설명·그림,
  질문 2개·선택지 4/3개, E 직접 답과 답한 카드의 뒤 그림 보존을 확인한다.
  스크린샷 폴더를 인자로 주면 정상 데스크톱/휴대폰 카드와 형식 경고·선택지 뒤 설명을 남긴다.
  고정 sleep·재시도·시한 연장은 없다.

- 첫 gate 도중 별도 Cargo 빌드를 겹치면 install 승인 검토 중 실행 파일이 교체될 수 있다.
  실제 install owner가 승인 발급에서 exit 4로 실패했고, 격리 사본을 질문 표시 사건 뒤 교체하면
  `installation_content_conflict`/4, 그대로 두면 0으로 제어 재현했다. 실행 시각과 실행 파일 교체 시각도 겹쳤다.
  최종 검증은 빌드를 끝내고 시험을 시작하며, 시험 중 같은 Cargo 실행 파일을 다시 빌드하지 않는다.
  승인·시한·기대값은 바꾸지 않는다.
- 병렬 전체 gate의 E2E68은 부하(load average 87 관찰) 중 안전 모드의 고정 3초 Refresh와 정상 listen이 경합했다.
  보관한 기록은 되돌리기 11:19:58Z·정상 listen 11:20:01.459Z·pane 20초 대기 실패다.
  다른 gate가 끝난 뒤 같은 시한으로 단독 실행해 8.5초에 통과했다. 관련 제품·시험 코드는 바꾸지 않았으며,
  당시 최종 전체 검증은 ci-local의 `--tee` 순차 단계를 써 Rust 시험과 웹 E2E의 부하를 나눴다.
  t-0797 이후 `--tee`는 기본 실행과 같은 병렬 경로를 쓴다.

## 입력 중 숨긴 게이지 자리 회수(t-0622, plan §8.398)

- 사용자 1A에 따라 숨긴 게이지의 폭과 보내기 단추 사이 간격을 글 칸에 돌려준다. 첫 입력·전체 삭제 때만 폭이 바뀌며, 글이 늘어 여러 줄로 접힐 때는 유지한다. 기존 §8.321의 자리 유지 단언은 이 선택으로 대체한다.
- E2E50의 실제 Claude 기록에서 보통(38%)·창 모름(84k) 게이지를 숨기면 폭이 0이고 글 칸이 게이지 폭과 간격만큼 넓어지는지 검사한다. 전체 삭제하면 원래 폭이 돌아오고 높은 사용량(86%)은 입력 중에도 폭을 유지한다. 기존 펼침·정리·iPad·휴대폰 및 700px 경계 검사를 유지한다.
- E2E66은 실제 usage가 있는 기록을 읽어 38% 게이지가 생긴 뒤 시작한다. Chromium·WebKit × 데스크톱·휴대폰 네 조합에서 빈 입력칸과 첫 입력의 폭 차이를 실제 게이지 폭·간격과 대조하고, 이전 폭에서 접히던 한글이 한 줄에 들어가는지 확인한다. 조합 입력 중·확정·긴 영문·붙여넣기·창 축소/회전·지우기의 native textarea 줄 수, 단추 옆 배치, focus와 전송 없음 검사를 유지한다. 지운 뒤에는 빈 입력칸의 폭과 줄 경계가 돌아와야 한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 50-context-gauge,66-composer-wrap --workers 1`로 실행한다. 입력·DOM·ResizeObserver 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 기존 E2E22·35·51과 전체 `nice -n 15 bash scripts/ci-local.sh`를 함께 확인한다.
- 수정 전 내장 웹에서 E2E66은 데스크톱 Chromium 약 73.89px·WebKit 약 74.67px의 미회수 공간과 이전 한글 첫 줄 경계의 줄바꿈으로 실패한다. 휴대폰에서는 이미 게이지가 없어 회수할 폭이 0이다. 수정 후에도 같은 폭·줄 경계 단언을 쓴다.

## 한도 초과 직접 시작(t-0619, plan §8.397)

착지 전체 검증은 E2E 정리 도구의 전체 `ps command` 출력이 Node의 1 MiB 버퍼를 넘는 ENOBUFS를 드러냈다.
전체 목록은 PID·부모·상태만 읽고, 명령줄은 실제 정리 후보 PID별로 조회한다. 기존 신원 재대조·부모 정지·자손 종료는 유지한다.
`node --test web/test/session-cleanup.test.mjs`는 관계없는 실제 프로세스 열 개의 argv 합계를 1 MiB보다 크게 만들며,
원래 도구의 ENOBUFS RED와 수정 도구의 session/writer 종료·관계없는 프로세스 보존 GREEN을 확인한다.
버퍼·대기 시한·시험 기대값은 늘리지 않는다.


- `cargo test -p aaa-host --lib over_limit_start_requires_a_direct_user_or_verified_coordinator --locked`는
  직접 CLI·웹과 다른 세션·작업자·자동화·빈/여러 줄/긴 근거를 구분한다.
- `cargo test -p aaa --test m13 explicit_over_limit_start_keeps_queue_waiting_and_requires_coordinator_user_request --locked`는
  실제 한도가 찬 상태에서 기본 시작 거부·CLI 강제 시작·coordinator pane의 근거 없는 거부와 명시 지시 시작을 검사한다.
  host 재시작 뒤 같은 요청자·시각을 보존하고, 강제 시작이 모두 자리를 반환할 때까지 예약은 시작하지 않는다.
- E2E47은 실제 host·agent와 Chromium 데스크톱·WebKit iPhone·iPad에서 추가 확인·돌아가기·실제 기동·출처와 표지·예약 대기를 확인하고
  `thread-<기기>-start-limit`·`over-confirm`·`over-started` 화면을 남긴다. 관측은 DOM·task 연결·ticker 완료이며 고정 sleep은 없다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`다. 기존 E2E 번호·시한·자동 예약 정책은 유지한다.
- 이 작업의 전체 실행에서는 기존 rollout transfer keeper가 원격 agent 교체 중 30초 사건 시한을 넘겼다.
  16코어에서 load average 139.27·126.31, 전송/사전 검사 99.382초였고 영속 작업은 오류 없이 Running이었다.
  같은 시한·단언의 단독 실행은 전체 100.04초·전송/사전 검사 65.354초로 통과했다.
  부하 근거와 단독 결과를 함께 기록하며 배포 로직·시험 시한은 바꾸지 않는다.

## thread 목록 상태 점 정렬(t-0620, plan §8.396)

- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 48 --workers 1`: 실제 coordinator·자동화·일반 thread를
  각각 선택하고 1280·820·390px에서 상태 칸 중심의 x 좌표가 0.5px 안에서 같은지 검사한다.
  실제 머신 연결은 유지한 채 표시 글만 짧고 긴 이름으로 바꾸어 내용이 상태 칸을 침범하지 않는지도 확인한다.
- 기존 자동화 막대·agent 표지 간격·휴대폰의 자동화 표지·규칙 이동 검사를 유지한다.
  선택 후 서랍과 DOM 사건을 기다리며 고정 sleep·재시도·시한 연장은 없다. `status-column-*.png`에 폭별 화면을 남긴다.
- 원래 데스크톱 CSS의 위치 기준을 다시 적용한 별도 실험실은 coordinator·자동화 305px, 일반 thread 339px로
  같은 정렬 단언에서 실패한다. 첫 수정의 휴대폰 선택 줄 10px 이동도 이 검사에서 잡아 메뉴의 flex 칸까지 고정했다.
- 모든 select가 같은 위치 기준과 상태 칸 여백을 가지며, 터치·좁은 창의 메뉴 칸은 선택 여부와 무관하게 확보한다.
  전체 검증은 `nice -n 15 bash scripts/ci-local.sh`다.


## install 승인 PTY의 종료 수명(t-0485, plan §8.395)

- `cargo test -p aaa --test install --locked`: 기존 승인·취소·worker 수명 단언을 유지한다. 실제 자식 종료 사건으로 master를 닫고 reader 완료까지 수집하며, 두 사건 모두 총 60초 안에 와야 한다. EOF만 온 뒤 살아 있는 자식도 무한 wait하지 않는다.
- `authorization::terminal_lifecycle::child_exit_closes_terminal_owner_before_eof_and_preserves_final_output`은 Unix의 실제 PTY 뒤에 소유자가 writer를 보관하는 실제 UnixStream을 잇는다. 실제 자식이 종료한 뒤에도 EOF가 안 오는 ConPTY 수명을 재현하며 종료 코드 7과 마지막 출력 300줄을 모두 검사한다. 자식 종료 때 master를 닫는 수리를 제거하면 60초 RED, 복구하면 GREEN이어야 한다. 이 keeper의 Unix cfg는 재현 fixture의 POSIX shell/UnixStream 때문이며 Windows의 기존 install owner들은 실제 ConPTY에서 같은 driver를 검증한다.
- Windows cross는 실행 증거가 아니다. macOS 로컬 통과와 macOS CI의 yes 뒤 정지·target 미상의 15분 종료를 구별하고, Windows/macOS 실제 CI 결과는 착지 뒤 감시 결과로 판정한다.

## 할 일 한 목록·예약 맨 위(t-0611, plan §8.393)

- `node web/e2e/web.e2e.mjs --spec 76-task-one-list --workers 1`: 자리 한도를 채운 실제 host에 CLI로 예약을 넣고, Chromium 1280px에서
  맨 위 예약 차례·하위 함께·확인 필요의 노란 표지와 상세의 까닭을 본다. 끌기로 예약 차례 바꾸기·일반 → 예약(내용 확인 뒤 그 자리)·
  예약 → 아래(풀기와 원래 자리 알림)를 하고 host `task queue` 차례와 대조한다. 「예약」 칩·저장소 거르기의 숨은 예약 줄, 거른 채
  Alt+↓, 새로고침 뒤 기억을 확인한다. 휴대폰(Chromium·WebKit 390px)은 맨 위 차례와 「순서」 ▲▼·⋯ 시트 풀기를 확인한다.
- `node --test web/test/tasksModel.test.mjs`의 「할 일 한 목록과 예약」은 행 배치(2A·crumb·숨은 예약 줄), 예약 순번(끌어 놓기·
  보이는 이웃과 바꾸기), 예약 푼 자리, 옛 `##` 틈의 옮기기 규칙을 검사한다. 43·46·67은 목록 머리 대신 한 목록 차례를 본다.
- DOM·host 기록으로 끝내며 고정 sleep·재시도·시한 연장은 없다.

## owner 목록 갱신과 저장소 선택 경계(t-0616, plan §8.392)

- main `35f6ce96`의 원래 E2E54 단독 실행은 3.7초에 통과했다(load average 약 6).
  같은 제품 실행 파일에서 실제 assignable 응답을 보류하고, 머신 목록 DOM 반영 직후 저장소 change를 보내면
  「owner 응답이 고른 저장소를 초기화했다」로 실패했다(load average 약 5). 부하만의 실패로 처리하지 않는다.
- `TaskDetail`의 owner 목록 effect는 예약한 render의 dirty 대신 상태 updater가 받은 최신 draft를 검사한다.
  저장소·제목 등 저장할 수정과 이름을 아직 안 쓴 사람 선택은 보존하고, 손대지 않은 폼만 다시 분류한다.
- E2E54는 실제 owner 응답·MutationObserver·두 렌더 frame으로 이 경계를 제어한 뒤 선택·저장 가능·파일 반영을 본다.
  고정 sleep·시한 증가·재시도를 쓰지 않는다. E2E46의 빈 사람 이름 보존과 E2E75의 연결 복귀도 함께 유지한다.

## 기동 선점·복구와 배포 진행(t-0606, plan §8.391)

- host `automation::start_tests::start_admission_owns_the_thread_before_recovery_can_observe_it`는
  실제 자동화 시작의 DB commit 직후, 첫 ThreadPrepare 전 복구기를 호출한다. 기존 코드는 새 Starting을
  중단된 기동으로 실패 처리했다. DB 행이 공개되기 전에 기동 task의 claim을 잡고, 실제 frame fixture의
  준비·기동 완료와 1회 실행까지 확인한다. 제어 고리는 host 단위 시험 빌드에만 있으며 고정 sleep은 없다.
- `task_queue::tests::recovery_reloads_under_the_claim_and_does_not_fail_a_completed_start`는
  복구기가 읽은 Starting 사본 뒤에 실제 기동 완료/송신이 확정되는 반대 순서를 검사한다. claim 아래에서
  최신 행을 다시 읽어 Open·pane·송신 기록을 보존한다. 기존 코드는 완료된 기동을 Failed로 바꿨다.
  `failed_start_admission_rolls_back_the_row_and_releases_the_claim`은 실제 INSERT 실패를 주입해
  새 행·자동화 영수증이 없고 claim도 반환됐는지 확인한다. 기존 중단·재기동 keeper도 유지한다.
- m13 rollout의 서비스 기록은 정확한 job ID의 `started_ms` 뒤에서 기존 30초를 잰다. host 확인 뒤의
  로컬 agent_restart는 host_restart와 같은 머신의 job을 고른다. 같은 phase의 마지막 항목은 원격
  agent일 수 있으므로 로컬 job 선택에 쓰지 않는다. 전송 재개·원격 복구·공유 host/agent 복구의
  네 로컬 기록 대기를 같은 경계로 맞추며, 순서·횟수·SHA·독립 PID·동일 job 재개 단언은 유지한다.
- host_update의 30초 진행 실패는 별도 계측에서 재관측했다. 당시 agent stack은 실행 파일 SHA-256
  검사 중이었다. 같은 약 194MB 파일의 debug 대조 3회에서 기존 경로 1.46~1.91초, ring 경로
  0.14~0.25초였고 해시는 모두 같았다. 샘플은 그 순간의 작업을 보여 주며 전체 지연의 유일한 원인이나
  OS 교착을 증명하지 않는다. `sample`을 붙인 실행의 시간과 일반 실행의 시간을 같은 표본으로 보지 않는다.
- `aaa-common::util::sha256_file`은 이미 사용하는 ring의 SHA-256으로 파일을 매번 64KiB씩 읽는다.
  agent 배포·독립 guard와 update/복구의 모든 기존 대조 지점을 유지하며 해시 cache·검사 생략은 없다.
  `sha256_file_matches_vectors_chunks_and_fresh_bytes`는 알려진 abc 값·빈 파일·블록 경계·끝 조각을
  독립 sha2 계산과 대조하고, 같은 경로의 같은 길이 변경·없는 파일 오류도 검사한다. 성능 합격 상한은 없다.

`cargo test -p aaa-host --lib --locked`, `cargo test -p aaa-common --lib --locked`,
`cargo test -p aaa --test m13 rollout:: --locked`, 기존 자동화 시작 m13 및 전체
`scripts/ci-local.sh`로 확인한다. 이 수리에서 추가 시한 연장·실패 재시도·m13 task_edit 및
resource_limits 변경은 하지 않는다.

고정 sleep 검사도 `cfg(test)`가 붙은 필드·초기화 뒤의 운영 코드를 시험으로 오인하지 않게
항목의 실제 범위를 읽는다. 필드·별칭·블록·여러 test 함수/모듈·generic/where 절 뒤의 경계를
회귀 시험으로 검사하며, 실제 시험 sleep은 계속 검출한다. 기존 파일별 기준값은 바꾸지 않는다.

## 할 일 로그·FD 회수의 CI 경쟁(t-0612, plan §8.390)

- `cargo test -p aaa --test m13 task_edit_keeps_bytes_conflicts_and_logs --locked -- --exact`: 직접 편집 fixture는
  같은 폴더의 임시 파일을 완성한 뒤 rename한다. 임시 파일 쓰기와 rename 사이에 실제 ticker tick을 끝내도
  마지막 변경은 「새 일」의 `파일 직접/update` 한 건이어야 하며, 원래 before bytes·충돌·권한 검사는 유지한다.
- 원래 `std::fs::write`의 truncate → ticker tick → write 순서를 제어하면 CI와 같은 「다른 일」의
  `파일 직접/add`가 첫 로그로 나와 RED다. 제품은 로그 저장 뒤 사건을 보내고 있었으며, 로그를 더 기다려도
  이미 생긴 삭제/추가 기록은 수정 기록이 되지 않는다. 제품의 기록 순서·감시 정책은 바꾸지 않는다.
- `cargo test -p aaa --test resource_limits --locked`: 실제 384개 HTTP 연결·OS fd 회수·EMFILE 복구를 유지한다.
  회수 대기의 여유 2개를 연결 수 단언에도 같은 상수로 적용한다. CI의 `400 → 18`은 원래 단언에서 RED,
  수정 뒤에는 `400 → 16·17·18`을 허용하고 `400 → 19`와 예전 256개 상한은 계속 거부한다.
- 고정 sleep·시험 재시도·시한 연장 없이 상태·사건으로 확인한다. 전체 검증은 `bash scripts/ci-local.sh`다.

## 사용자 지시의 task 모델·머신 변경(t-0609, plan §8.389)

- `cargo test -p aaa-host --lib task_queue::tests:: --locked`: 명시 지시의 빈 근거·제어 문자·RawSave·여러 동작 거부,
  기본 모델/자동 머신 변경과 예약 ID·순서·생성 시각·감사 보존을 검사한다. 승인 후 다른 내용이 바뀌었거나
  Starting/Running·기동 이력이 있는 Check 상태이면 거부하며 원문과 선택 출처를 보존한다.
  예약 뒤 기본 모델이 달라져도 머신만 변경하면 예약 당시 profile·model·effort·설정 판을 유지한다.
  새 기본 모델 회귀는 수정 전 claude → codex의 의도하지 않은 변경에서 RED, 수정 뒤 같은 단언에서 GREEN을 확인한다.
- `cargo test -p aaa --test m13 coordinator_user_request_changes_choices_without_losing_reservations --locked -- --exact`:
  격리 host·agent의 실제 coordinator pane에서 CLI를 실행한다. 근거 없는 모델 변경, 빈 근거, 다른 칸 혼합,
  모르는 profile, 시작/재시작 우회를 거부한다. 명시 지시 뒤 미기동 예약의 새 선택·실행 승인판과
  ID·생성 시각·순서·취소 상태·변경 전후 기록을 확인한다. host 재시작 뒤 기존 자리 resolve 사건만으로
  바뀐 profile·자동 머신의 실제 pane이 시작되어야 한다. 고정 sleep·재시도는 없다.
- 첫 전체 gate에서 E2E49는 host 기억을 본 직후 화면의 옛 본문을 읽었고, E2E74는 휴대폰 iframe에서 그림을
  누른 뒤 Escape가 크게 보기에 전달되지 않았다. 49는 같은 본문·thread 단언을 DOM 갱신 사건까지 기다리고,
  74의 휴대폰은 기존 E2E59와 같은 닫기 단추 tap을 쓴다. 데스크톱 Escape 회귀는 E2E59가 유지한다.
  원본 실패 로그를 남기고 두 keeper를 좁게 검증한 뒤 최종 전체 gate를 실행한다. 시한·기대값은 바꾸지 않는다.
- 착지 통합의 E2E46은 새 owner 응답이 아직 이름이 빈 사람 선택을 초기화해 입력칸이 사라졌다.
  load average 62~64였으나, 한 worker에서 owner 요청을 사람 선택 뒤로 보류해 같은 결함을 재현했다.
  이름을 쓰기 전의 사람 선택도 편집 중으로 보존한다. E2E46은 실제 응답과 두 렌더 frame 뒤 이름 입력·거부 검사를
  유지해 RED/GREEN을 확인한다. 부하만의 실패로 넘기거나 시한·기대값을 바꾸지 않는다.
- 기존 `task_selected_model_and_machine_are_changed_only_by_user`·직접 파일 수정·기본 모델 보호 keeper와
  coordinator 안내의 새 생성/doctor 갱신 검사를 유지한다. 전체 정본은 `nice -n 15 bash scripts/ci-local.sh`다.

## 할 일 머신 연결 목록(t-0600, plan §8.388)

- `node web/e2e/web.e2e.mjs --spec 75-task-owners --workers 1`: 실제 실험실 agent를 끊은 동안 할 일을 처음 열고,
  Chromium 1280px·WebKit 390px에서 `labA(끊김)`과 선택 불가를 확인한다. agent를 다시 붙이면 화면을 새로 열지 않아도
  머신 선택이 살아나며, 고치던 제목은 남아야 한다. 두 번째 끊김·복귀도 같은 상세에서 확인한다.
- 상세를 닫았다 열기·머신 칸 focus·바로 시작마다 실제 `assignable` HTTP 응답을 기다린다. 모델·머신 칸은 받은 목록만
  바꾸고 폼은 다시 만들지 않는다. 통계·pane 변화는 재조회 원인이 아니며, 새 조회나 화면 닫기 뒤에 온 옛 응답은 버린다.
- 연결과 DOM·HTTP 사건으로 끝내며 고정 sleep·재시도·시한 연장은 없다. 수정 전 코드는 첫 복귀의 머신 연결 단언에서 실패한다.

## 웹 알림함 숨기기(t-0590, plan §8.382)

- `node web/e2e/web.e2e.mjs --spec 26-activity-bar,53-thread-questions,56-operate,57-legacy-special-panes,62-asks-close`: 활동 막대에 알림함 단추·배지·패널이 없고, 예전 `inbox` 패널 기억으로 새로 열어도 「작업」이 보인다. 기존 26의 데스크톱·좁은 창·휴대폰 세로/가로·두 테마 배치 검사가 바닥 사용량 칸을 그대로 확인한다.
- main 통합으로 더해진 E2E23의 종료 문서 API keeper는 숨긴 알림함 대신 「답할 것」 조회를 쓴다. 정상 상태·복귀 뒤 두 실제 응답과 종료 뒤 전송 없음 단언을 유지한다.
- 53은 확인 필요 post의 「답할 것」 배지·본문·답함과 coordinator inbox 기록을 계속 확인한다. 56은 조작·보기 기기와 휴대폰의 알림함 숨김, 기존 CLI 조회·끝내기·끝낸 기록을 확인한다. 화면·host 응답을 기다리며 고정 sleep을 추가하지 않는다.

## 배포 조각의 개발 빌드 비용(t-0592)

- `cargo run --locked -p aaa-proto --example deploy_codec`: 실제 배포 응답의256KiB base64 JSON 조각을
  256번씩 encode/decode하고 원문 bytes를 대조한다. 성공 표본3개의 시간을 출력하며 성능 통과 상한·sleep·재시도는 없다.
- 기존 `sha2` 설정에 더해 dev/test의 `base64`·`serde_json`을 opt-level3으로 컴파일한다. debug assertions·release 설정·조각 크기·
  실제 배포 실행 파일·SHA·영수증·복구·재시작 owner를 유지한다. 프로토콜 crate 전체를 최적화하지 않는다.
- 전후 비교는 같은 머신에서 같은 `rollout::rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart`
  keeper를 실행하고 빌드와 시험 시간을 나눈다. codec 측정과 전체 gate·다른 머신의 결과를 같은 개선률로 쓰지 않는다.

## 웹 E2E 병렬 작업(t-0592)

- 37-chat-pages의 Chromium·WebKit ×1280·390·820px ×Claude·Codex 조합12개는 각각 새 실험실에서
  실행한다. source의 기존 단계 이름을 정확히 하나씩 선택하고 모든 조합의 위치·가상화·새 기록·복원 검사를 유지한다.
  `--grep`은 그 조합만 고르고 `--workers 1`은 이전처럼 한 실험실에서 모든 단계를 차례로 실행한다.
- 상태를 이어 쓰는23-composer-attachments 등의 spec은 분할하지 않는다. 공통 준비의 `always`도 유지한다.
  `node --test web/test/e2e-jobs.test.mjs`는 원 source의12개 조합이 반복마다 정확히 한 번 선택되는지,
  의존 단계 보존·grep·문장 부호·시간 합계를 확인한다. 실패 재시도는 없다.
- 기본 작업 수는 10 CPU 이상이면 5/8 내림·최대 10개, 그 밖은 CPU 둘마다 하나다. GitHub macOS job은 2개를 명시한다(t-0830). 명시한 `AAA_E2E_WORKERS`와 `--workers`는 그대로 적용한다.
  요약의 전체 시간은 wall time이며 분할 spec의 행은 독립 작업 점유 시간의 합계라고 표시한다.
  단계·전체 시한이나 실패 판정은 바꾸지 않는다. 같은 실행 파일을 고정한 전후 측정과 전체 정본으로 검증한다.

## 종료 시작에 완료된 조회(t-0592)

- E2E23의 「종료 시작의 완료 응답」은 실제 WebKit/App의 projects HTTP 응답을 받은 뒤 JSON 완료만
  beforeunload까지 보류한다. 실제 reload에서 pagehide보다 먼저 routines를 시작하는 경계를 검사한다.
  기존 코드는 `beforeunload → routines fetch → cancelled → pagehide`로 실패한다. 원래 간헐 실패의
  access-control 문구와 이 제어 재현의 cancelled 기록은 구별하며, 종료 중 실제 fetch 발생 자체를 단언한다.
- 공통 API는 beforeunload부터 새 일반 요청을 AbortError로 취소한다. 기존 pagehide·pageshow·명시 keepalive
  경계도 유지한다. 새 문서의 실제 routines 응답을 확인해 조회를 영구 차단하는 변형은 통과하지 못한다.
- `web/test/apiRequest.test.mjs`는 두 종료 사건부터 네트워크 시작0, 명시 keepalive, pageshow의 새 요청과
  취소한 요청 비재생을 검사한다. 일반 API 오류를 성공으로 바꾸거나 pageerror를 무시하지 않는다.

## 전체 검증 시간 기록

같은 머신·캐시 조건끼리 비교한다. 머신은 공개용 별칭이며 `mac-a`는 같은 16 CPU macOS 머신,
`mac-b`는 10 CPU·32GiB macOS 머신이다. 두 머신의 수치를 직접 비교하지 않는다.
부하는 시작 직전 `uptime`의 1분 평균이다. 단계는 서로 겹치므로 합산하지 않는다.
`캐시 준비`는 같은 main의 전체 빌드·시험 산출물이 있는 상태다. 첫 실행의 준비 비용을 코드 개선 효과로 세지 않는다.

| 날짜 | main SHA·후보 | 머신 | 시작 부하 | 조건·결과 | 전체 | web | fmt | build | clippy | Rust | 웹 E2E | public |
| --- | --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2026-10-21 | `35ecad05` | mac-a | 미기록 | 이전 착지 보고·통과 | 195.99 | 5 | 1 | 22 | 12 | 166 | 165 | 1 |
| 2026-10-22 | `94d2081e` | mac-a | 10.73 | 새 작업 폴더·통과 | 203 | 4 | 1 | 27 | 15 | 137 | 169 | 1 |
| 2026-10-22 | `94d2081e` | mac-a | 20.41 | 캐시 준비·변경 전·통과 | 174 | 3 | 1 | 1 | 1 | 141 | 166 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 첫 후보 | mac-a | 21.06 | 캐시 준비·E2E47 정리 경합 실패 | 179 | 4 | 1 | 2 | 2 | 127 | 170 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 정리 수리·worker8 | mac-a | 12.81 | staging 뒤 재빌드·diff 임시 폴더 충돌 실패 | 203 | 4 | 1 | 25 | 13 | 127 | 171 | 1 |
| 2026-10-22 | `94d2081e` + t-0670 충돌 수리·worker10 | mac-a | 7.06 | 빌드 산출물 재사용·전체 통과 | 164 | 4 | 1 | 7 | 5 | 130 | 150 | 1 |
| 2026-10-23 | `eaebe576` | mac-b | 4.74 | 새 작업 폴더·전체 통과 | 438 | 9 | 2 | 51 | 31 | 319 | 374 | 1 |
| 2026-10-23 | `eaebe576` | mac-b | 11.28 | 캐시 준비·변경 전·전체 통과 | 385 | 8 | 1 | 5 | 1 | 304 | 368 | 2 |
| 2026-10-23 | `eaebe576` + t-0810 첫 후보 | mac-b | 3.79 | worker7·E2E52 이미지 대기 실패 | 375 | 8 | 1 | 4 | 8 | 359 | 323 | 2 |
| 2026-10-23 | `eaebe576` + t-0810 최종 후보 | mac-b | 2.46 | worker6·이미지 load 대기 수리·전체 통과 | 355 | 8 | 1 | 4 | 10 | 324 | 339 | 2 |

단위는 초다. 이전 기록 이후 웹 시험은 73→85파일로 늘었지만 첫 E2E 시간은 165→169초이고,
Rust 단계는 166→137초였다. 코드·캐시·부하가 다르므로 이를 특정 병합의 증감으로 단정하지 않는다.
새 작업 폴더의 준비(web·fmt·build)는 32초, 같은 코드의 준비만 다시 잰 값은 7초였다.
시스템 인덱싱·분석 프로세스 부하는 있었고 다른 thread의 무거운 빌드는 관측되지 않았다.

같은 index의 캐시 준비 기준선174초와 worker10 후보164초를 비교하면 Rust141→130초·웹166→150초다.
Rust 실행기 자체는131.24→120.09초, install 시작 대기는90.73→68.58초였다.
worker10에서도 Rust의 가장 긴 묶음은 m13-5(88.02초)이고, 가장 긴 단일 웹 spec은11-agent-panes(64.5초)다.
37-chat-pages의12작업 합계는 단일 spec의 wall time과 구별한다. 산출물21개·m13 107개·doctest를 유지하고,
Rust 시험은1057→1058통과(새 회귀 시험1개)·기존 ignored3개, Python19개 중18통과·기존 Linux 전용1skip,
웹85파일·96작업 전체 통과다. 새 작업 폴더의203초와164초 차이를 모두 코드 개선으로 세지 않는다.

E2E47 실패는 한도 초과로 웹에서 시작한 thread의 task 연결을 기동 완료로 간주하고 바로 정리한 시험 순서였다.
보존한 실패 DB는 `status=open`, `brief=pending`이고 시작 작업 잠금이 남아 resolve를 거부했다.
기존 출처·재확인·예약 대기 단언 뒤 실제 `brief=delivered`를 관측하고 resolve를 한 번 호출한다.
실패 재시도·시한 변경·단언 삭제 없이 정리의 선행 조건을 기다린다.

다음 gate의 agent diff 두 실패는 임시 index의 `PID + 순번` 이름이 이전 프로세스 잔여 폴더와 충돌한
`File exists`였다. 비교마다 새 ID를 써 한 번 생성하며 기존 폴더는 지우거나 재사용하지 않는다.
`cargo test -p aaa-agent --lib diff::tests:: --locked`의 새
`stale_pid_temp_index_never_blocks_a_new_process`는 격리 TMP 환경의 새 자식에 옛 PID·첫 순번 폴더와
보존 파일을 먼저 둔다. 원래 코드는 같은 오류로 실패하고, 수정 뒤 실제 diff와 보존 파일 바이트를 검사한다.
큰 patch·외부 callback·index 바이트/시각·ref/작업 파일 비변경 단언도 유지한다.

### 독립 Python 검사와 웹 작업 배치(t-0810)

Unix의 test 단계는 Python 검사와 Rust runner를 함께 시작하고 둘 다 끝날 때까지 기다린다.
Python 원문은 `python.log`에 보존하고 test 로그에도 넣는다. 어느 쪽이 실패하든 단계는 실패하며
실패한 검사 이름·종료 코드를 마지막에 남긴다. Windows의 기존 전체 Cargo 시험 경로는 유지한다.
`python3 -m unittest scripts.test_ci_local scripts.test_rust_tests`는 실제 셸의 Python·Rust 도구 자식을
같은 socket에서 만나게 한다. 기존 직렬 실행은 두 번째 자식의 진입 사건에서 실패한다.
동시 진입·양쪽 완료·각각의 실패 전파·로그 원문·public 후행과 기존 CPU 예산·선택 완전성을 검사한다.
고정 sleep·시험 재시도·시한 변경은 없다.

웹 기본 worker는 10 CPU 이상에서 CPU 수의 5/8을 내림하되 최대10개다(10 CPU는5→6개,
16 CPU 이상은 기존10개). 10 CPU 미만은 기존 core 둘마다 하나이며 명시한 worker 값도 유지한다.
모든 spec·기존 독립 단계 분할·실험실 격리와 성공 조건을 유지한다. 같은 머신의 변경 전 전체 실행이
모두 통과한 기록으로 Rust binary·웹 spec 배치 시간을 갱신한다. m13 우선·분할·CPU 예산은 유지한다.

새 머신의 첫 실행438초와 캐시 준비385초의 차이는 개선 효과로 세지 않는다. 첫 Python30개는29.86초,
캐시 준비 실행에서는20.12초 동안 Rust에 선행했다. 기록이 없던 restart가 install보다 먼저 배치됐고
캐시 준비 install은 대기79.22초+실행204.24초였다. 웹92파일·103작업은368초였다.
이 머신에서는 다른 무거운 빌드를 관측하지 않았으나 시스템 인덱싱 부하가 있었고,
host 연결 오류로 thread 목록은 확인하지 못했다. GitHub job 시간은 인증된 머신에서만 조회하므로 생략했다.

이전 표 이후 설정·휴대폰 경로의 검사 범위가 늘었다. `ccc4a985`의 설정 본문·화면 조합,
`987b7079`·`eaebe576`의 뒤로/앞으로·설정 복귀·메뉴 선택 경계가 실제 spec에 추가돼 있다.
다른 머신의 옛 시간만으로 병합별 증가 초를 확정하지 않는다. 제품 동작 검사를 삭제하지 않는다.

첫 후보의 worker7에서는 웹368→323초였지만 Rust304→359초·Python20.12→66.00초로 느려졌다.
시작 부하3.79에서 실행 중32.44까지 올랐고, 관측한 상위 CPU 사용자는 이번 Rust·브라우저 자식이었다.
메모리 압력 조회는 여유87%였다. 전체375초는 E2E52 실패 실행이므로 성공한 개선 수치로 세지 않는다.
최종 후보는 10 CPU에서 worker6으로 줄여 두 단계의 경합을 확인했다.

E2E52는 첨부 줄이 저장된 뒤 실제 그림의 `complete && naturalWidth > 0`을 기다리다 실패했다.
기존 `domIs`는 MutationObserver만 구독해, DOM이 만들어진 뒤 도착하는 이미지 load를 놓칠 수 있었다.
새 E2E52 대조는 별도 페이지의 실제 PNG 응답을 대기 함수 등록 뒤 전달하고 다른 DOM 변경을 만들지 않는다.
기존 helper는 같은20초 오류로 실패한다. `domIs`에 load/error capture 구독을 더하고 완료·실패 때 해제한다.
기존 그림 표시 단언·20초 상한·모바일/패드 밝음/어두움 조합은 유지한다. 최초 대조 fixture는 URL의
중복 슬래시로 이미지 요청 gate에 오지 못해120초 준비 실패였으며 재현 증거에서 제외했다.
URL을 `new URL`로 고친 대조의20초 실패와 helper 수정 뒤 결과를 구분한다.
수정 뒤 대조는52ms·E2E52 전체는16.5초로 통과했다.

캐시 준비 기준선385초와 최종355초를 비교하면 전체30초(7.8%)·웹368→339초이며,
Rust 단계는304→324초로 늘었다. Rust runner 자체는283.78→323.52초,
install은 대기79.22→80.70초·실행204.24→242.63초여서 병렬 부하의 비용이 남아 있다.
같은 명령·nice·캐시 준비 조건의 한 쌍이며 시작 부하11.28/2.46은 다르다. 평균 절감량으로 일반화하지 않는다.
3분 목표는 미달이며 현재355초에서175초를 더 줄여야 한다. worker 증가만으로 해결됐다고 보지 않는다.

네 전체 실행 모두 Rust29묶음·doctest의1162통과·기존 ignored6개를 유지했다.
최종 Python33개 중32통과·기존 Linux 전용1skip, front 유닛489통과,
웹92파일·103작업 전체 통과이며 의존 경계와 공개 검사도 포함한다. 시험 삭제·단언 완화는 없다.
최종 긴 Rust 작업은 install242.63초·m13-4 235.66초·m13-6 229.98초다.
웹 단일 파일은39-phone-back 103.4초·66-composer-wrap 90.2초·23-composer-attachments 90.1초가 길다.
37-chat-pages 144.9초는12개 독립 작업의 합계이며 단일 파일의 wall time이 아니다.

## Rust 전체 시험 병렬 실행(t-0591)

Unix의 `bash scripts/ci-local.sh --only test`는 Cargo `--workspace --no-run`의 시험 실행 파일 전체를 준비한 뒤
기본 여섯 프로세스로 실행한다. m13은 `--list`의 정확한 전체 이름을 rollout 단독과 나머지 세 묶음으로 나누고,
분할 전후 이름 집합·중복과 각 자식의 실제 실행 수를 대조한다. 작업 디렉터리는 각 package의 manifest 폴더다.
모든 binary와 workspace doctest를 유지하고 실패 뒤에도 나머지 작업을 끝낸다. Windows는 기존 Cargo 실행을 유지한다.
`AAA_RUST_TEST_WORKERS`로 프로세스 수를 정하고 `RUST_TEST_THREADS`가 없으면 CPU 수를 `min(프로세스 수, 4)`로 나눈다.
작업은 `min(libtest thread 수, exact 선택 시험 수)`만큼 CPU 자리를 사용하며 전체 합계를 CPU 수 이내로 제한한다.
16코어의 기본 m13 여섯 작업은 rollout 세 개 각1자리·나머지 세 묶음 각4자리로 함께 시작한다.
명시한 thread 수는 유지한다. `--logs` 아래 `rust/plan.json`·`summary.json`·작업별 로그에 실행 목록·시간·결과와
작업의 시작·대기 시간·CPU 자리 수가 남는다.

`scripts/rust-test-durations.json`은 같은 머신의 전체 검증에서 잰 binary 실행 시간이다.
m13 여섯 묶음을 먼저 시작하고, 나머지는 이 기록의 긴 binary부터 실행한다. 새 binary는 기록이 없으므로 먼저 실행한다.
시간 기록은 순서에만 쓰고 시험 선택·단언·시한에는 쓰지 않는다. doctest도 기존처럼 모두 실행한다.
부모가 CPU 자리가 있는 작업만 worker에 보내므로 큰 작업의 자리 대기가 worker를 점유하지 않는다.
앞의 큰 작업이 들어갈 수 있으면 우선하고, 들어갈 수 없으면 뒤의 작은 작업을 빈 CPU 자리에서 실행한다.
`test_waiting_large_job_does_not_block_a_fitting_job`은 실제 자식 A(2자리)와 D(1자리)가 만나야 열리는 socket으로
CPU 3자리·worker 3개에서 B·C(각2자리)의 대기가 D를 막지 않는지 검사한다. 기존 실행기는 이 사건에 도달하지 못한다.

`python3 -m unittest scripts.test_rust_tests -v`는 목록 완전성, 실제 두 자식의 동시 진입,
실패 뒤 후속 실행, 선택한 시험이 0건으로 빠지는 오류, Cargo 산출물부터 doctest까지의 실패 전파를 확인한다.
실제 자식 여섯이 CPU15자리 안에서 함께 만나는지, 실패 뒤에도 예산을 반환하고 다음 작업을 실행하는지도 본다.
가짜 자식은 socket 사건으로 만나며 고정 sleep을 쓰지 않는다.

`cargo test -p aaa --test m13 rollout:: --locked`의 macOS owner 세 개는 독립 사본 host·TLS agent를 사용한다.
전송·사전 검사·같은 job 재개, 거부된 갱신·원격 복구, 공유 host/agent 복구·진행 중 취소를 나눠 실행한다.
앞 시나리오의 배포를 반복해 준비하는 대신 필요한 서명 이미지로 시작하며 기존 단언·시한은 유지한다.
완료 rollout 뒤 새 기록의 진입·옛 이력·job ID 비재사용은 host 단위 시험
`completed_rollout_allows_the_next_record_without_reusing_jobs_or_losing_history`가 소유한다.

## 공개 검사 크기 조회 일괄화(t-0591)

`python3 -m unittest scripts.test_check_public -v`는 실제 임시 Git 저장소에서 기존 금지 확장자·이력·정확한 크기 예외를
유지하고, 공백·개행·탭·한글·선행 대시 파일명, index를 갱신하지 않은 작업 파일 크기, symlink 대상·삭제·끊긴 링크를
확인한다. 비 UTF-8 파일명은 이를 허용하는 Linux에서 바이트 그대로 검사한다(macOS 파일시스템은 생성부터 거부한다).
symlink 순환으로 실제 metadata 조회를 실패시켜 빈 크기 목록으로 통과하지 않는지도 본다.

크기 조회는 파일마다 `wc`를 띄우는 대신 Python 한 번의 `stat`으로 읽는다. NUL 파일 목록·작업 파일 기준·기존 크기 상한과
내용·커밋·세션 검사는 유지한다. 속도 비교는 같은 worktree·index·환경·기준 commit의 전체 스크립트를 전후 실행하고
종료 코드·출력을 함께 비교한다. 성능 수치로 시험 기대값이나 시한을 정하지 않는다. 전체 정본은 `bash scripts/ci-local.sh`다.

## 보고서 폴더 fetch·음원(t-0589, plan §8.381)

- `cargo test -p aaa-host --lib --locked`: `report_connections_have_one_exact_origin_and_capability_prefix`는
  loopback·HTTPS의 정확한 보고서 경로와 헤더 삽입 차단을 검사한다. `signed_report_http_boundary`는 기존
  폴더 밖·숨은 파일·symlink·위조 링크 거부를 유지하고 새 폴더 읽기 범위를 검사한다. 기존 connect-src none 단언은
  요구된 폴더 읽기 계약으로 바꾸며 범용 self·별표로 완화하지 않는다.
- `report_sandbox_warning_does_not_fail_copy_or_repeat_for_the_same_snapshot`은 실제 보고 회수로 제한 API 경고가
  inbox에 한 번 남고 복사 성공은 보존되는지 본다. 문자열 탐지는 별도 unit에서 상대 fetch·외부/동적 경로·제한 API를 검사한다.
  `publishing_reports_warns_for_restricted_apis_in_copies_and_live_sources`는 publish의 사본·실시간 등록 경고와 정상 상대 fetch를 검사한다.
- `cargo test -p aaa --test m13 library_view --locked -- --exact`: host 사본·agent 스트리밍·Range·내려받기·기존 경로 거부와
  Origin null의 CORS·쿠키 동반 거부·다른 Origin에 CORS 없음.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 71 --workers 1`: 격리 host·agent에서 390×844 Chromium·WebKit의
  앱 iframe과 인증 없는 공유 탭을 연다. 사용자 누름에서 WAV 둘을 fetch·decode·GainNode로 혼합하고 analyser 신호·running 상태·
  두 source의 ended를 확인한다. 출력 끝에는 gain 0을 두어 실제 스피커로 소리를 내지 않는다. HEAD·Range·하위 HTML의 상대 읽기,
  다른 보고서·정규화/인코딩 경로 탈출·앱 API·외부 서버·쓰기·credentials·쿠키·저장소·부모 접근 거부와 링크 폐기도 검사한다.
  외부 요청은 실험실 서버의 받은 수 0으로 확인한다. 고정 sleep·timeout 연장·실제 로그인·실제 iPhone 청취는 없다.

제품 변형 대조: connect-src를 host 전체로 넓히면 다른 보고서 읽기가 성공해 격리 단언에서 실패하고,
Access-Control-Allow-Origin을 없애면 음원 읽기 전에 fetch 실패를 검출한다. 두 변형 뒤 제품 파일 bytes를 복구하고 정본에서 다시 검사한다.

## 종료 문서의 앱 API(t-0485)

`node web/e2e/web.e2e.mjs --spec 23 --grep '종료 문서 API:'`는 실제 host/App/WebKit에서 정상 tasks/asks
조회 응답을 확인한 뒤 pagehide·WS 갱신 사건을 보내 일반 API 네트워크 시작이0인지 검사한다. pageshow 뒤 같은
두 실제 조회도 확인한다. 원래 코드에서는 tasks/inbox 두 fetch를 잡아 RED, 공통 apiFetch 뒤 GREEN이었다. t-0590에서 알림함 조회를 없앤 뒤에는 같은 경계를 tasks/asks 실제 조회로 유지한다.
개별 컴포넌트에 시험 export/guard를 붙이지 않는다. 새 web/aaa 빌드 뒤 E2E23 파일 전체와 전체 gate를 확인한다.

공통 경계는 post·인증·업로드·사용량·진단 API에 적용하며, 종료 뒤 취소는 AbortError다. 취소한 쓰기를 복귀 때
재생하지 않는다. 일반 숨긴 탭은 차단하지 않고 명시 keepalive는 유지한다. 떠날 때 설정 보존은 기존 E2E14가 맡는다.

## install I6 CI 시험 터미널(t-0485)

- `cargo test -p aaa --test install --locked`: 실제 승인·마법사·worker·TLS 등록 owner를 세 OS에서 실행한다.
  직접 연 시험 PTY도 aaa-vt로 ConPTY의 DSR/DA/색 질의에 답하며 기존30초 시한·승인 단언을 유지한다.
- `authorization::terminal_answers_cursor_query_before_application_prompt`: POSIX 실제 PTY의 자식이 DSR 응답 전에는
  승인 물음을 내지 않는다. 응답기 없는 원래 driver는30초 RED, 응답기 연결 뒤0.03초 GREEN이다. 셸/stty/dd fixture만
  Unix cfg이고 Windows 기존 owner는 실제 ConPTY 시작 질의로 같은 연결을 검증한다.
- `grant::verified_host_grant_uses_tls_pin_and_keeps_consumption_bound_to_same_machine`: TLS/grant/pin/재조회·폐기 단언을
  유지하고, host runtime을 내린 뒤 lab을 지운다. 종료를 제거하면 남은 host 소유자 단언이 실패해야 한다.
- Windows `install::engine::input::password::tests::console_masks_and_restores_after_success_cancel_and_eof`:
  같은 시험 터미널 응답기로 시작 질의를 통과하고 실제 console 암호 비노출·원래 mode·취소/EOF를 검사한다.

Windows 실행은 main push 뒤 CI로 판정한다. cross clippy/build를 실제 실행으로 부르지 않는다. macOS worker의
시한 초과는 출력 단계와 부하 근거를 함께 남기며 시한 연장·기대값 변경·시험 삭제로 통과시키지 않는다.

## 할 일 표시 ID(t-0575)

- `cargo test -p aaa-host --lib display_ids --locked`: 기존 ULID·CRLF를 가진 파일을 여덟 조회가 동시에
  이주해도 같은 `d-NNNN`을 받는지 본다. 삭제·로그 정리·옛 원문 복원·DB 다시 열기에도 대응을 유지한다.
- `cargo test -p aaa --test m13 task_display_ids_survive_rename_move_restart_delete_and_archive --locked -- --exact`:
  실제 host·agent·CLI에서 괄호 제목과 명시 owner의 시작/연결, 제목 변경·목록/하위 이동·기록 조회,
  삭제 복원·번호 비재사용·예약 순서/취소·host 재시작·완료 보관 복원을 검사한다. ID와 ULID를 모두 대조한다.
- `node web/e2e/web.e2e.mjs --spec 70-task-ids`: 실제 host의 첫 이주가 주석 외의 bytes를 유지하는지,
  웹 편집 뒤 CLI 선택자가 같은 할 일을 고치는지, host 재시작 뒤 320/390/768px에서 ID 검색·표시와
  가로 넘침을 검사한다. CLI 응답·DOM 사건·연결 준비로 끝낸다.

기존 원문 바이트 keeper는 이주가 끝난 고정 ULID fixture로 내용·충돌·이력을 검사한다. 기존 웹 내용 keeper는
식별 주석을 제외한 내용을 비교하고, 직접 편집 fixture에서는 주석을 보존한다. 식별 주석 자체의 정확한 bytes와
번호·내부 ULID 대응은 위 전용 keeper가 검사한다. `parent_attention_is_published_after_the_verdict_is_saved`는
자동 정리의 파일 사건 뒤 판정 확정 사건도 보내는지 검사한다. 파일 쓰기 직후의 읽기가 판정 전 원문을 받더라도
마지막 사건에서는 상위의 확인 필요 상태가 확정돼 있어야 한다. ID를 없애거나 재발급하는 변형은 전용 keeper에서 실패해야 한다.

## thread 재시작의 pane 교체(t-0567)

- `cargo test -p aaa --test m13 restart_replaces_exited_panes_locally_and_remotely --locked -- --exact`:
  실제 host·로컬 UDS/원격 TLS agent에서 kill 뒤 profile 변경, 자발적 exit 뒤 새 대화·resume·직접 pane resume를
  연속 실행한다. thread 결속, 옛 pane 제거, 이름·탭·화면 추적 ID, 다른 실행 중 pane 보존을 본다.
  host 재시작 뒤 두 agent의 Hello에도 옛 pane이 돌아오면 실패한다. host의 임시 TLS 포트는 재시작 전에 고정한다.
- `restart_spawn_failure_keeps_the_old_pane_until_a_successful_replacement`: 존재하지 않는 fixture 실행 파일로 기동을
  실패시켜 옛 pane·layout 보존을 확인한 뒤 설정을 고친다. 성공한 교체 뒤에만 옛 기록이 없어져야 한다.
- 기존 `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`:
  로컬·원격의 고정 Claude 계정에서 Codex profile 변경은 기본 Codex 계정이 있어도 거부하고 brief bytes·profile·pane·계정을
  보존한다. 같은 harness의 다른 profile은 원래 인증으로 기동하고 옛 pane을 지운다. 가짜 인증·socket gate만 쓴다.
- 기존 `restart_with_another_profile`, `memory_and_restart`, `reboot_resumes_threads`, 계정 pane 재개·복원 keeper가
  실행 중 거부, brief 다시 쓰기, 저장 대화 이어가기와 재부팅 경계를 맡는다. 사건·출력·세션 상태로 기다린다.

수정 전 첫 keeper는 `labA/profile: 옛 pane이 기타로 남았다`에서 실패한다. pane 교체 누락·계정 사전 검사 누락·
일반 thread의 pane resume 우회를 각각 되심으면 해당 owner 단언이 실패해야 한다. 제품과 시험의 시한은 늘리지 않는다.

## 계정별 사용량(A2, t-0561)

- `cargo test -p aaa-common --lib quota`: 공식 camelCase RPC·Claude 상태줄 fixture, 누락·NaN·범위 밖
  수치·기간 모름·모델별 창·창별 관측 시각·2분/10분·리셋·미래 시각 경계.
- `cargo test -p aaa-host --lib quota`: 가짜 공식 실행 파일 여섯을 동시에 사용한다. RPC 기록은
  initialize·initialized·account/rateLimits/read만이며 thread/start·turn/start는 0회여야 한다.
  401·429와 재조회 가능 시각·깨진 응답·누락·stderr·알림·응답(버킷 이름 포함)의 canary 폐기·로컬 갱신 거부·임시 홈 정리를 본다.
  병합 규칙은 동시 관측 충돌·모델 창 분리·다른 계정/로그인 세대 거부·끊김의 원래 시각 보존·머신 시각 차이를 본다.
- `cargo test -p aaa-agent --lib quota`: 전용 pane 파일과 창별 시각·세대·경로 검사·0600·원문 폐기.
  기존 `accounts::encrypted_accounts_reject_tampering_and_lock_without_stopping_other_accounts`는
  세대 변조 거부·잠김·다시 등록·같은 별칭 삭제/재등록의 새 세대도 검사한다.
  `cargo test -p aaa-host --lib legacy_vault`는 A2 전 AAD로 만든 가짜 암호문을 읽고 첫 재암호화에도 로그인 세대가 유지됨을 확인한다.
- `node web/e2e/web.e2e.mjs --spec 65-account-quota`: 실제 host·agent와 가짜 공식 RPC 두 계정,
  관측 없는 Claude, 계정 pane의 주입된 상태줄 → agent snapshot → host 카드, 누락 주간 창의 원래 시각,
  401·429·잠긴 가짜 금고·agent 끊김·마지막 숫자·backoff 중 추가 RPC 없음·로그인 세대 교체·Reader 비노출·320/390/768px 카드.
  가짜 Claude pane에 F6 사건을 보내 창을 바꾸고 출력 표지를 기다린다. sleep·실제 로그인·실제 인증은 없다.
  `AAA_TEST_USAGE_REFRESH_MS=0`은 기존 수동 조회 하한 시험 고리이며 429의 시각은 줄이지 않는다.

`AAA_USAGE_HOME`을 지정한 기존 실험실에서는 새 수집기도 실제 Codex를 실행하지 않는다.
debug 실험실이 `AAA_TEST_QUOTA_CODEX`에 명시한 가짜 실행 파일만 허용한다. RPC 단위 시험은 실행 파일을
직접 주입하므로 전역 env를 바꾸지 않는다. 원격 숫자는 수신 시각 대신 원래 창 관측 시각으로 판정한다.

`quota_rpc::tests::refresh_refusal_reads_the_post_body_before_closing`는 가짜 갱신 POST의
`Expect: 100-continue` 응답을 받은 뒤 본문을 보내고 최종 401과 응답 끝까지 읽는다.
본문을 받기 전에 TCP를 닫던 서버는 첫 응답이 401이라 실패한다. 일반 갱신 fixture의
전송 오류를 HTTP 오류로 완화하거나 재시도하지 않는다. 여섯 모드의 실패에는 mode·snapshot과
가짜 Python 예외 종류/errno만 남기며 원문 인증·응답·stderr는 기록하지 않는다.

무작업 RPC keeper에는 실제 `thread/start` 전송을 추가하는 변형을 심어 RPC 기록 단언이 실패함을 확인한다.
변형 뒤 제품 파일 bytes를 원복하며 원복한 후보에서 전체 검증한다.

## host 로그인·기본 계정(t-0510)

`cargo test -p aaa --test m13 accounts::`는 실제 인증을 읽지 않고 가짜 공식 CLI·토큰·갱신 서버만 쓴다.
기존 keeper가 로컬/원격 네 Codex 작업의 기본 계정 전달·동시 회전·기본 해제 뒤 pinned resume와 모든 기록 비노출을 확인한다.
`host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`는 공식 로그인 argv·격리 홈·원본 출력 차단,
임시 인증 정리, 새 worker/자동화 평가/원격 작업의 기본 선택, 기존 로컬 작업 유지, 명시 계정 우선, 깨진 기본 거부,
coordinator의 host 재시작 뒤 refresh·재부팅 복원·pane 재개와 같은 기록 홈을 확인한다.
동시 coordinator pane 재개는 Barrier 뒤 두 실제 CLI를 경주시켜 성공이 하나인지 확인한다.
준비 중 중복 거부는 `cargo test -p aaa-host --lib account_policy::tests::preparing_account_resume_rejects_duplicates_before_agent_spawn -- --exact`가 맡는다.
실제 `SpawnAccount` 전송을 받은 뒤 응답을 보류하고, 둘째 기동 요청 없이 거부되는지와 첫 응답 뒤 새 pane 결속·옛 pane 재거부를 확인한다.
CLI 버전 확인의 제품 제한시간(5초)을 시험 제어로 소진하지 않는다. 이전 시험의 gate 해제를 정상 호출 종료 사건 뒤로 늦추면
CI와 같은 성공 0개·기동 실패/중복 거부가 재현된다. 준비 중 claim 검사를 제거한 변형은 새 host owner가 중복 기동 요청에서 실패해야 한다.
인증 worker의 pane 재개는 저장한 thread launch·identity를 이어 쓰고, 실행 중 thread restart가 같은 홈을 다시 쓰지 않는지 본다.
로그인 실패·취소·host 정상 종료는 SIGHUP 무시 자식·PTY를 가진 자손을 포함해 종료와 폴더 정리를 기다린다. 가짜 자식 안에서 계정 관리 명령을
실행해 aaa 세션에서는 로그인·기본 변경·삭제가 거부되는지도 확인한다.

`parallel_host_logins_finish_independently_and_cancel_returns_after_cleanup`는 가짜 Claude/Codex를 동시에 보류한다.
한쪽 성공/실패의 독립 종료, 이미 끝난 자식의 상태 정리, 같은 ID 중복 cancel과 다른 ID cancel의 동시 호출,
두 로그인 중 host 정상 종료를 검사한다. cancel 응답 시 최종 상태·자식 PID 소멸·임시 홈 정리를 확인한다.
원래 비동기 취소 응답은 waiting을 반환해 이 keeper가 실패한다.

완료는 socket gate·CLI 상태·세션 종료 사건으로 판정한다. token은 해시만 비교하고 마지막에 DB/WAL·journal·
상태 폴더·로그의 원문/8바이트 조각/인코딩 변형을 검사한다. 원래 코드에서 이 keeper는 로컬 profile의 공식 인증 env가
host 실행 기록에 평문으로 저장되는 경로를 잡았다. host가 실행 환경을 저장하기 전에 가림 규칙을 등록해야 한다.
기본 선택 제거·로그인 원본 출력 노출·coordinator 인증 없이 재개·준비 중 인증 참조 허용 변형은 각각 이 keeper를 실패시켜야 한다.
실제 host 브라우저 인증은 배포 뒤 사용자가 확인하며 이 가짜 시험이 대신 보증하지 않는다.

## host·agent 재시작과 상태 복원(t-0491)

`cargo test -p aaa-host --lib transitions::tests::hello_replay_does_not_publish_idle_between_panes -- --exact`
는 Hello의 첫 pane Working 사건을 처리하는 실제 대기자 콜백에서 ticker를 실행한다.
뒤 pane의 상태를 아직 재생하지 않았다는 이유로 `쉼(no agent)` inbox를 만들거나 조회 묶음을 쉼으로 바꾸면 실패한다.
로컬·원격을 모두 검사하며, Hello가 끝난 뒤 실제 Idle은 한 번 알린다. 복원 울타리를 재부팅 pane에만 치는
원래 결함을 되살리면 같은 거짓 쉼 알림에서 실패해야 한다. 시간·부하·새 production 주입 지점을 쓰지 않는다.

`cargo test -p aaa --test e2e restart::restart_preserves_working_pty_without_input_signal_or_resize -- --exact`
는 격리 host, 로컬 UDS agent, 원격 TLS agent를 띄운다. 별도 FIFO로 진행하는 raw PTY harness가
host → 로컬 → 원격 순서의 SIGTERM 재시작 중에도 checkpoint를 기록해야 한다. 자식·session host PID,
수신 바이트 전체, SIGINT·SIGTERM·SIGHUP·SIGWINCH, 실제 PTY 크기와 Working 상태를 비교한다.
재연결 뒤 의도한 `p` probe를 **PTY에서 읽은 사건**을 기다려 앞서 큐에 든 예상 밖 입력도 검사한다.
Screen 응답은 PTY writer 완료의 울타리가 아니다. 마지막 실제 Esc는 Idle과 정확한 바이트를 확인하는 양성 대조군이다.
session 재연결에 Esc를 넣는 변형은 `local-back: inputs` 단언에서 실패해야 한다.

이 시험은 연결·PTY 생명주기 계약을 지킨다. 실제 Codex의 네트워크 요청, hook 실패, TUI 표시까지
대신 검증하지 않는다. 턴이 중단됐다는 진단은 화면 관측과 rollout의 후속 실행·`turn_aborted`·턴 경계를 함께 대조한다.
끝 20줄의 `esc to interrupt` 부재나 짧은 화면 무변화만으로 중단을 확정하지 않는다. 긴 추론 요약이 그 줄을 대신하는
실측 화면에서도 도구 실행은 이어졌다. 연결 직후 `no agent`는 상태 복원이 끝났는지도 먼저 확인한다.

## agent 재연결 중 상태 전달(t-0508)

`cargo test -p aaa-agent --lib link::post_tests::harness_changes_while_waiting_for_welcome_reach_the_host -- --exact`
는 실제 link 세션의 Hello를 받은 뒤 Welcome을 보류한다. 그 사이 기억한 Idle이 연결 뒤 전달되어야 한다.
Hello를 읽는 host에는 바뀐 사건만, 옛 host에는 최신 기억 전체를 보내며 Ping/Pong으로 앞선 전달의 끝을 확인한다.
연결 뒤 같은 밀리초의 새 Working도 뒤에 전달한다. `harness_hello.v1`이면 변경분을 생략하는 원래 결함을
되심으면 Idle 누락 단언에서 실패해야 한다. 새 production 시험 주입점은 없다.

기존 `cargo test -p aaa --test e2e stale_working_left_by_hooks_turns_idle_from_the_screen -- --exact`
는 재시작한 실제 agent의 연결 하나만 실험실 UDS로 중계한다. Hello의 옛 Working을 확인하고, 실제 감지기가
`harness.json`에 새 Idle을 적을 때까지 Welcome을 보류한 뒤 연다. host의 새 Idle 사건과 파일 기억을 모두 검사한다.
시간 상한·감지 간격은 그대로다. 원래 제품과 변경분 전송을 제거한 변형은 같은 host Idle 대기에서 실패해야 한다.
전송 계약의 세부 표는 link 시험이 맡고, 이 E2E는 화면 감지·저장·프로세스 재시작 경계를 맡는다.

`link::post_tests::live_hook_waits_for_the_replay_to_finish`는 작은 실제 duplex 전송 버퍼와 여러 pane의
기억 fixture로 replay를 멈춘 동안 실제 `prompt_submit` 전달을 진행한다. 재생 뒤 live hook이 빠짐없이 오고,
없는 세션의 실제 Screen 요청도 NotFound 응답을 받아야 한다.
상대가 Ping 뒤 write half만 닫으면 읽기 half가 남아도 세션이 끝나야 한다. 잠금 제거·replay와 reader 직렬화 변형은
각각 live 누락·EOF 대기에서 실패한다. 디스크 저장을 이 시험에서 반복하지 않는다.
`tests::pending_host_send_lets_go_when_the_connection_changes`는 옛 bounded 큐를 가득 둔 채 연결을 바꿔
옛 전송 대기가 끝나고 새 전송이 가는지 본다. 연결 변경 시 취소를 빼면 옛 전송 대기에서 실패한다.

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

## 제거된 특수 기능

임무 실행·reset·평가자·특수 패널 전용 시험은 A4에서 제품 계약과 함께 제거했다. 파일 전송과 A1/A2 keeper는 아래 「자동화 A4」의 owner로 옮겼다.

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

### 배포 후보의 바이트 보존(t-0525)

`cargo test -p aaa-host --lib link::post_tests::deployment_bytes_survive_auth_redaction_and_a_partial_tail -- --exact`
는 실제 host Agent link reader·요청 대기자를 작은 duplex 버퍼로 연결한다. 가짜 인증값의 원문과 인코딩을
온전한 청크와 마지막 3,176바이트 부분 청크에 넣고, 모든 조각의 offset·길이·EOF와 합친 바이트의 SHA를 검사한다.
EOF·요청 응답·연결 종료 사건으로 끝나며 고정 sleep, 실제 인증, 운영 서비스, 새 production 주입점은 쓰지 않는다.
같은 연결의 파일 보기 응답은 계속 인증 조각을 가려야 한다.
수정 전 host의 일반 JSON 가림은 배포 B64까지 디코딩해 바꾸므로 길이가 같은 채 EOF SHA 단언이 실패한다.
`DeployChunk`의 가림 예외만 제거한 변형도 같은 단언에서 실패해야 한다. agent의 청크 쓰기·sync·EOF 검사와
기존 rollout 설치·재시작 E2E는 그대로 둔다.

### 인증 가림의 B64·실패 처리·청크 경계(t-0525)

`cargo test -p aaa-common --lib redact::tests::`는 실제 protocol `B64`의 인코딩 글자에만 인증 조각이
겹치는 경우와 실제 바이트에 비밀이 있는 경우(8바이트 부분 포함)를 구분한다. 일반 글이 우연히 B64처럼 보여도
원문 비밀과 8바이트 조각의 짧은 인코딩도 가려야 한다. B64는 serde newtype 이름으로 구별하고 JSON wire·TS 모양은 그대로 둔다.
형식을 엄격히 검사하는 값과 직렬화 오류는 panic·원문 반환 없이 오류여야 하며, 오류에 비밀을 넣지 않고 기존
안전한 기록 파일도 덮지 않아야 한다. 실패 이유만 남기고 agent 송신 메시지·host 수신 연결·Front 요청을 거부하거나
Front 응답을 안전한 오류로 바꾼다. thread·journal 기록은 원문을 쓰지 않는다.

원래 B64 글자 가림은 `Invalid symbol 42` panic에서 RED였다. 원문 fallback·B64 타입 구분 제거·옛 스트림 복원
변형은 각각 이 keeper에서 실패해야 한다. 스트림은 비밀과 CSI ANSI를 섞은 입력의 모든 청크 경계에서 같은 가림과
끝 flush를 내야 한다. 원문과 가림 표시를 분리해 겹치는 인증 창의 끝을 보존하며, 단독 ESC도 다음 청크까지 잡는다.
기존 agent 연결 변경 시험은 snapshot 응답으로, host 배포 시험은 일반 파일 보기 응답과 뒤따르는 Pong으로
실제 송수신자가 B64 충돌 뒤에도 살아 있음을 검사한다. 실제 계정·로그인·갱신·기록 검증은 기존 m13 accounts keeper가 맡는다.

### macOS 시험 PTY의 마지막 출력(t-0525)

`cargo test -p aaa --test m13 terminal_capture_keeps_output_after_stdio_close_before_reading -- --exact`
는 작은 C 자식이 stdout·stderr에 글과 ANSI를 쓰고 표준 FD를 모두 닫았다는 파일 사건 뒤에 reader를 시작한다.
기존 helper는 exit 0·출력 0바이트로 RED다. macOS helper는 exec 전에 `/dev/tty`를 한 번 열어 controlling TTY의
참조를 잡고 닫는다. XNU의 종료 drain이 master 수집을 기다리므로 늦은 reader도 모든 바이트를 받아야 한다.
이 준비만 제거한 변형은 같은 단언에서 실패해야 한다. timeout·원래 profile 제거 출력 단언은 그대로다.
이 CLI 성공 출력은 `println!`→직접 PTY reader 경로이며 Agent link·session-host 가림을 지나지 않는다.
커널 경계 근거: [cttyopen의 참조](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty_tty.c),
[종료 시 drain·revoke](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_exit.c),
[ttyclose의 flush](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c).

### update 후보 서명과 사람 터미널 출력(t-0488)

- 기존 E2E `update_replaces_keeps_and_rolls_back`, m13 `coordinator_deployment_is_gated_and_survives_backend_restarts`, `rollout::`의 전송·원격 복구·공유 복구 owner 세 개가 실제 다음 실행·교체·보관·rollback과 재시작 수명을 계속 소유한다. macOS 후보는 바이트를 덧붙이지 않고 서로 다른 ad-hoc 서명 식별자를 써서 같은 version·다른 SHA·유효 strict 서명을 갖는다. 복사·서명은 자식 프로세스에서 끝내 병렬 fork에 쓰기 FD를 물려주지 않는다.
- m13 `web_pair_requires_a_person_outside_aaa_panes`의 사람 문은 그대로 검사한다. 터미널 helper는 직접 PTY를 열고 남은 출력을 EOF까지 읽은 뒤 자식을 수거한다. macOS `script`가 자식 종료를 먼저 처리하면 종료 0인데도 PTY의 끝 출력을 잃는다. 전달 프로세스 정지→자식 출력·종료→kqueue 종료 확인→전달 프로세스 재개 순서로 고정 sleep 없이 재현했다.
- helper의 cwd는 이전 `script`처럼 호출자의 작업 폴더로 명시한다. PTY의 HOME 기본 cwd를 쓰면 아직 없는 격리 HOME을 넘긴 service 시험이 실행 전에 실패한다. 기존 `service_stop_start_restart_wrap_launchctl`이 이 경계를 지킨다.
- ca58f1b0 macOS CI의 pair 실패는 outside 출력 전에 `pane 안 pairing 끝`에서 났다. worker의 brief 글과 Enter 사이에 probe를 보내면 두 글이 붙어 probe가 실행되지 않는다. 시작 응답을 brief 완료로 보지 않고, 시작 전 구독한 Thread 사건에서 brief 수신과 fixture의 보고 처리를 기다린 뒤 probe를 보낸다. 같은 typing 경계·느린 제출에서 수정 전 실패/수정 후 통과를 보고, 준비 대기를 제거한 변형도 실패해야 한다. 사람 문 단언과 30초 상한은 그대로이며 실패에는 역할·파일·pane·화면 진단을 남긴다.
- rollout의 서비스 재시작 기록은 해당 단계가 시작된 뒤 기다린다. 전송 완료만 기다린 뒤 두 사전 검사·설치·서비스를 한 30초 안에 기대하면 부하 아래 설치 중에 실패했다. 같은 rollout의 내구 cursor 전진/단계 시작을 `rollout_changed` 사건으로 관측하고 단계별 기존 상한을 적용한다. 같은 cursor·같은 시작 표지의 반복 사건은 상한을 새로 시작하지 않는다. 서비스 기록의 순서·횟수·실제 job/복구 단언은 그대로다. 재시작 대신 상태만 조회하는 제품 변형에서 기존 서비스 기록 keeper가 실패해야 한다. 자동 복구도 감시의 `restore_started`와 파일 복구 완료(`restarting`·`awaiting_old`)를 먼저 확인한 뒤 서비스 기록을 기다린다. 옛 판 프로세스 기동부터 SHA 대조·파일 복구·재시작을 한 서비스 대기에 섞지 않는다. 각 기존 30초 상한과 서비스 순서·횟수·정확 baseline SHA·독립 PID·새 프로세스 확인 단언은 유지한다.
- 구 inode 설치 경로, 같은 SHA의 후보 둘, PTY 출력 유실을 각각 심어 기존 keeper가 실패하는지 확인하고 바이트 단위로 원복한다. 시간 제한·사람 문·교체 기대값은 바꾸지 않는다.
- macOS 제품 update는 self-check에 사용한 staging과 별도로 실행 이력 없는 설치 파일을 독점 생성한다. 검증한 SHA와 복사한 바이트가 같은지 확인한 뒤 rename하고, 원래 실행 파일은 그대로 보관한다. 유효 서명 후보의 교체 완료 뒤 다음 실행이 SIGKILL 9(`EXEC/SECURITY_POLICY`)로 죽는 경계를 수리한다. [Apple의 갱신 원칙](https://developer.apple.com/documentation/security/updating-mac-software)은 파일에 붙은 서명 캐시 때문에 새 파일로 교체하라고 설명한다. 구 경로도 rename은 했으므로 이번 관측을 단순 제자리 덮어쓰기나 INVALID_PAGE라고 부르지 않는다. 구 staging inode 재사용을 심으면 기존 update keeper의 다음 실행이 다시 실패하고, 원복하면 전체 교체·rollback이 통과해야 한다.

### 흐름 사건의 동시 발행 제어(t-0488)

- m13 `flow_events_go_out_in_seq_order`는 기존 `AAA_TEST_FLOW_HOLD` 경로를 Unix 제어 연결로 쓴다. host가 DB 저장 뒤 `Hhold: 켬`, 다음 사건이 순서 잠금을 기다릴 때 `Wfree: 켬`을 보낸다. 시험이 해제 `R`을 보내야 첫 사건을 발행한다. 고정 sleep·파일 polling·시간 경과에 따른 조용한 해제는 없다. 30초 실패 상한은 그대로이며 단계·연결·사건 출처를 실패 출력에 남긴다.
- Front 요청·조회·구독 연결을 미리 열어 두고, 저장은 붙잡힘 알림 뒤 한 번 조회한다. 기존 contiguous seq 및 수신 순서 단언을 유지한다. 구 시험은 hold와 잠금을 함께 제거해도 통과했다. 새 시험은 hold 제거를 조기 응답으로, 잠금 제거를 실제 수신 순서 `[2, 1]`로 잡아야 한다. 변형을 바이트 단위로 원복하고 runtime worker 하나에서도 통과시킨다.
- 제보된 ci-local의 첫 저장 30초 실패는 좁은 동시 부하 대조에서 재현되지 않았다. 이 수리는 확인한 제어 수명·관측의 허점을 없애며, 당시 지연을 특정 OS·DB 오류로 단정하지 않는다. 최종 검증은 test·e2e·clippy를 함께 도는 ci-local 전체다.
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

## 옛 임무 입력 안내·권한 시험

임무의 전용 입력 안내·추가 허용 목록·빈 입력 실행 순서는 A4에서 실행 코드와 함께 제거했다. 일반 파일 전송의 경로·hash·symlink와 구 Agent link 호환은 아래 A4 owner를 따른다. 과거 권한 실험은 plan §8.246·§8.251의 이력이다.

## resolve 뒤 종료된 pane 제거(§8.254)

- m13 `resolve_removes_exited_panes_even_when_the_worktree_is_kept_or_adopted`가 실제 CLI→host→agent 경계에서 dir·dirty worktree·adopted pane을 입력으로 exit 1 종료한다. 종료 상태 확인 뒤 resolve하고 pane 목록·layout에서의 제거와 폴더 보존을 단언한다. 새 신호·고정 sleep·production 시험 seam은 없다.
- 기존 `resolve_keeps_dirty_worktree_and_unmerged_branch`는 partial·skip-copy를 포함한 폴더/branch 보존과 별개로 pane을 닫는지 본다. `thread_adopt_existing_pane`은 복원된 살아 있는 pane도 닫고 원래 branch·파일은 남기는 경계를 본다. linger 시험은 기존대로 session host의 종료 유예를 기다리지 않는지 확인한다.

## 삭제된 TUI의 접속 거절

`cargo test -p aaa --test tui_removed`는 host 없는 로컬 `attach`·`tui`가 TUI 삭제 안내와 실패 코드로 끝나는지, 실제 host socket이 옛 `tui`·`tui@…` hello를 거절하고 CLI 상태 조회를 계속 받는지 본다. 폐지된 `mission`·`evaluator`·`evaluation`은 접속 전에 자동화 변환 안내로 끝나는지 같은 owner가 검사한다. 웹 E2E 01의 「옛 TUI WebSocket 거절·웹 API 유지」는 인증한 브라우저에서 옛 `/api/v1/front`에 접속해 분명한 Reject frame을 받고 일반 웹 API가 정상인지 본다. 웹 터미널·Ctrl+Enter·입출력 회귀는 기존 웹 E2E와 host·CLI 시험이 계속 맡는다.

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
- 안내: m13 `coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`·`project_lifecycle`이 `aaa post user`·`aaa project pause|resume`·`aaa project sweep`처럼 이름 없는 안내를 본다.

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

- `cargo test -p aaa-common ci::`: gh 304 exit 1·200/403 HTTP 해석, GET·머리 주입 거부, 응답 필드 축약, SHA의 일부 workflow 대기·취소·재실행·stuck, 실패 집합·병합 번호 계약을 본다. 실패 로그 표지는 실제 windows-2025 줄 모양(job·단계·시각 앞머리, `\` 경로)을 7바이트 조각으로 먹여 꼬리 밖 실패·panic 열 번호 제외·끝나지 않은 시험을 보고, 같은 job·단계의 다른 시험이 다른 실패로 갈리는지 본다(0a8c294f).
- `cargo test -p aaa --test m13 ci_conditional -- --nocapture`: 실제 host·agent와 가짜 gh, 승인된 임무를 띄운다. 기준 초록 → 304 무음 → 빨강의 CI 항목·임무 inbox와 깨움 한 번 → host 재시작 중복 방지·created pagination → 같은 실패 집합 무깨움 → 같은 job·단계의 다른 실패 시험은 같은 항목에 덧붙이고 다시 깨움(가짜 gh가 실패 줄을 32KiB 꼬리 밖에 둔다) → 꺼진 동안 실패 후 회복·CI fixed 무깨움 → 403 outage 한 건을 본다. 로그 잘림과 한 push의 여러 주인 번호도 이 경계에서 본다.
- 실험실의 기존 `AAA_TEST_ROUTINE_SECONDS=1`이 있을 때만 `ci_poll_secs` 하한을 1초로 낮춘다(운영은 30초). 고정 sleep을 넣지 않는다. 새 판정은 flow, 아무 일 없음은 다음 304 두 번(앞 CI pass 완료)과 ticker tick 뒤 확인한다.
- 결함 심기는 HTTP 상태보다 exit를 먼저 거부하기, 같은 실패 집합의 깨움 억제를 제거하기, 회복 자체가 임무를 깨우게 만들기 세 가지를 독립적으로 심는다. t-0685는 실패 시험 비교 빼기(「같은 실패 집합」으로 건너뜀)와 agent가 남긴 꼬리만 읽기(꼬리 밖 실패 시험 없음) 두 가지를 더 심었다. 해당 keeper가 의도한 이유로 실패한 로그를 보존하고 바이트 단위 복구 후 정상 통과를 확인한다.
- 운영 host·로그인·설정·실제 GitHub 쓰기는 건드리지 않는다. 배포 뒤 사용량·깨움 수 관찰은 coordinator의 별도 운영 단계다.

## 공용 watch 추출(t-0420 L1)

- 기존 `cargo test -p aaa-host ci:: --lib`와 `cargo test -p aaa --test m13 ci_conditional -- --nocapture`를 그대로 실행한다. E2E의 CI 사건·재시작·같은 실패·회복·outage 단언은 바꾸지 않는다.
- `l0_saved_state_survives_watch_extraction`은 L0 저장 문서 fixture를 실제 DB에 넣고 새 상태로 읽고 쓴 JSON 의미가 같은지 본다. cursor·SHA 판정 이력·미전달 outbox를 보존하고, 임무가 없을 때도 저장 입력이 남는지 확인한다.
- 결함은 각각 독립적으로 ① 프로세스 첫 조회의 따라잡기 누락 ② 전달 입력의 내구 key를 매번 바꿈 ③ source 상태의 최상위 펼침 누락을 심는다. 최근 poll 직후 새 host의 따라잡기 단위·pending 중복 단위·L0 저장 문서 keeper의 RED를 보존하고 바이트 복구 후 정상 gate를 실행한다.
- 첫 조회 표지 제거 변형은 기존 E2E에서 재시작 시간 자체가 단절 창을 넘어 통과했다. 이 감도 부족을 보존하고, 같은 DB의 새 host가 poll 1ms 뒤에도 따라잡는 keeper를 추가했다.
- 전체 gate는 `bash scripts/ci-local.sh`와 `bash scripts/check-public.sh`다. L1은 CI 동작·설정 변경 없이 구조만 추출한다.

## 배포 자동화 L0·L1(§8.276)

- host `rollout::tests::transition_receipts_require_success_hash_and_new_process`는 단계 전이 표, 실패·중단 중지, 영수증 ID/요청/소속, SHA·판과 새 프로세스 판정의 owner다.
- macOS m13 `rollout::`의 전송·원격 복구·공유 복구 owner 세 개는 격리 사본 host·TLS agent 둘과 기록하는 launchctl로 승인 거부, 사전 SHA 불일치 무교체, 모든 대상 dry-run 뒤 host부터 순차 실행, 실제 host/agent 프로세스 재시작 뒤 같은 job ID 유지, 안내 갱신·coordinator pane PID 지속을 확인한다. 서비스 호출 기록·연결 사건·출력 표지로 끝내며 고정 sleep은 없다. 실제 운영 launchd에 닿지 않는다.
- `python3 -m unittest scripts.test_deploy_build`는 임시 origin/main과 기록하는 빌드 도구로 조상 거부·격리·빌드 1회·salt·provenance를 확인한다. Unix 공식 ci-local test 단계에 포함한다.
- t-0547의 같은 시험은 기본 `target/deploy-build`가 `TMPDIR`·공용 Cargo target과 무관한지,
  명시 부모의 절대/호출 폴더 기준 상대 경로·공백·빌드마다 새 source/target을 확인한다.
  npm·Cargo·판 검증 실패를 각각 한 번 일으켜 worktree/중간 파일 정리·이웃 파일 보존과 기존 부분 출력 보존을 검사한다.
  부모가 파일이면 빌드 전 실패해야 하며 temp로 우회하지 않는다. 원래 temp 선택을 되살리면 기본 위치 단언에서 실패해야 한다.
  실제 macOS SIGKILL 사유는 가짜 도구 시험이 보증하지 않으며, 해당 머신의 빌드/OS 로그 대조와 구분한다.
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

SHA 거부 대기(t-0587)는 로컬 agent의 연결과 원격 `AgentUpdate` 단계 진입을 구분한다. 연결만 보고
`deploy status --wait`의 30초를 시작하면 로컬 재시작의 신원 대조·감시 해제 시간까지 함께 잰다.
원본 실패 기록에서는 원격 단계가 대기 시작 약 10.4초 뒤, Recovering이 약 27.1초 뒤에 시작했다.
기존 단계 진행 사건으로 원격 교체 진입을 확인한 뒤 원래 30초 상한으로 종료 표지를 기다린다.
SHA 오류·실패 cursor·앞 머신 SHA·서비스 호출 횟수 단언은 유지한다. 별도로 영속 guard의
`install_started=true`·`activated=false`·baseline SHA·Cancelled·무재시작을 확인하므로
BeginInstall 뒤 거부된 설치가 교체 lease 확인을 거쳐 Stopped로 끝나는 경로도 빠지지 않는다.
실제 설치 중 safety 철회의 늦은 rename·복구 keeper와 제품 시한·전이는 바꾸지 않는다.


## 순수 TOML·공통 자동화(L4)

`cargo test -p aaa-host automation::`는 실제 파일 파싱·스키마·안전한 본문 경로·이름 충돌·원본 보존·미리보기 hash·Reader 거부와 공통 큐의 예산·전달 모름을 검사한다.
CI/Git의 source outbox·중복 억제·재시작·outage는 기존 owner를 유지한다. 상주 실행과 reset은 A4에서 제거했으며 옛 파일·저장 형식은 변환 owner로만 읽는다.

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

### E2E37 실패 산출물(t-0501)

- 성공할 때는 진단 파일·추가 출력·추가 HTTP 요청·대기가 없다. 앞 기록 요청/응답과 논리적인 DOM 전이는 256개까지 메모리에 모은다. DOM 전이는 기존 두 animation frame 대기의 반환값으로 가져오며 frame·브라우저 왕복을 더하지 않는다. 가상 본문 mount 때마다 행을 세거나 높이를 재지 않는다.
- 실패 지점은 기존 터미널 진단이 화면을 움직이기 전에 저장한다. 실험실 `diagnostics/pages-<browser>-<width>-<harness>/`의 `failure.json`에는 시작·실패 시각, 원래 오류, 요청 cursor·응답 before/history·첫/끝 ID·행 수·HTTP 수신 여부와 DOM 전이가 있다. `dom.json`은 실패 당시 HTML·위치·처음 표지 좌표와 남은 DOM 전이, `screen.png`는 화면, `host.log`·`agent.log`는 해당 조합 시작 이후 로그와 앞 다섯 줄이다. 인증 설정·DB·cookie는 복사하지 않고 실험실의 시작 fingerprint도 가린다.
- renderer가 응답하지 않아도 먼저 받은 요청/응답·DOM 전이와 로그는 남는다. DOM·화면 수집이 실패하면 각각 `dom-error.txt`·`screen-error.txt`를 남긴다. 이 수집에만 별도 상한을 둔다(DOM은 기존 `inTime`의 5초 인수·6초 Node 울타리, 화면은 5초). 시험의 기존 제한·단언은 바뀌지 않는다. 단계 전체의 120초 실패도 같은 진단을 실행한다.
- `bash scripts/ci-local.sh --only e2e --logs <폴더>`는 실패 진단을 `<폴더>/e2e-failures/aaw-<pid>/`에 복사하고 `e2e.log`에 경로를 남긴다. GitHub의 같은 명령(`--logs .`)은 통과·실패 모두 `e2e.log`·`e2e-binary.log`·`workspace-summary.tsv`와 `e2e-failures/`(시간 기록 포함)를 `web-e2e-result-<run>-<attempt>` artifact로 14일 보존한다. 실험실 전체는 업로드하지 않는다.
- 산출물 점검은 첫 조합의 `처음 도달` 직후 예외를 한 번 주입해 공식 ci-local의 해당 E2E만 실패시킨다. 요청/응답·2,400행/처음이다 전이·DOM·PNG·host/agent 구간이 로그 폴더에 복사됐는지 본 뒤 소스를 bytes로 복원한다. 이 변형은 진단 경로의 검증이며 원래 간헐 실패의 재현이 아니다. 복원한 소스에서 전체 ci-local·check-public을 통과해야 한다. 다음 실제 실패의 artifact로 별도 원인 수리 thread를 연다.

### macOS 27 WebKit 끝 이동 경쟁(t-0474)

- 별도로 남겼던 `기록 2800` locator 30초 실패는 제품의 끝 이동 경쟁이었다. 원본 전체 ci-local에서 WebKit 1280 Claude가 재현됐고, 2801행 수신·오류 없음·끝 세 행은 빈 가상 row인 상태였다. scrollTop 129322 / scrollHeight 334056 / clientHeight 794로 화면이 끝에서 이탈했다.
- 실제 scroll 추적은 smooth 요청 → 첫 이동 565px → row 실측의 즉시 끝 보정 337516px → 옛 애니메이션의 다음 scroll 23190px 순서를 보였다. 제품이 마지막 사건을 손의 위 이동으로 오인해 따라가기를 껐다. Safari에서 과거 대화를 읽다 「맨 아래로」를 누르면 새 답이 도착해도 본문이 화면 밖에 남을 수 있었다.
- 가상화하는 긴 대화의 끝 단추는 즉시 이동한다. 같은 가상화 판정을 row와 끝 이동에 함께 쓰며, 실측마다 실제 끝을 맞춘다. 목적지가 바뀌는 native smooth 이동과 즉시 보정을 섞지 않는다. 새 애니메이션 상태나 테스트 전용 제품 표지는 두지 않는다.
- 회귀 소유자는 기존 E2E37의 12조합이다. 2400행 처음 도달·400행 gap·새 메시지 본문과 끝 따라가기·prepend 1px·300개 본문 상한·pane 복원·End 검사를 그대로 쓴다. 원본 실패와 수리 뒤 같은 전체 ci-local 조건의 3회 연속 결과를 보존한다. 각 실행은 새 웹 번들·실행 파일을 확인하고 Rust 시험·clippy와 다른 E2E를 함께 실행한다. 한 번이라도 실패하면 반복을 멈추고 원인을 조사한다. timeout·재시도·허용 오차·조합은 바꾸지 않는다.

## 휴대폰 뒤로 기록(t-0466, plan §8.301)

- `node web/e2e/web.e2e.mjs --spec 39-phone-back`: WebKit iPhone 390·430 × Safari 탭·홈 화면 앱 흉내에서 pane 셋과 대화의 보고서를 연 뒤 앱 문서 칸(init script가 적은 `history.length`)까지 `history.back()`을 popstate 사건마다 하고 화면 차례를 단언한다. 왼쪽 가장자리 쓸기는 `document.createTouch`로 touchstart·touchmove(frame마다)·touchend/touchcancel을 내고, 겹친 뒤로는 손가락을 뗀 뒤 popstate를 기다린다. 닫힌 pane 칸·「모든 thread」 닫기의 기록 칸 수와 휴대폰·데스크톱 빈 화면 문구도 본다. 고정 sleep은 없고 popstate·DOM·animation frame·drawer transition 사건으로 끝낸다.
- t-0466 수정 전 코드로는 첫 단계가 「(주소 없음)+빈 화면」으로 실패했다. 당시 쓸기 겹침의 「목록 닫힘」 규칙은 t-0541에서 상태 유지로 바뀌었고, 사용자 10-22 결정(t-0780)으로 다시 항상 닫기로 바뀌었다. 뒤로 화면 순서·빈 화면 방지·기록 칸 수 단언은 유지한다.
- t-0780(plan §8.340): 휴대폰 뒤로·앞으로는 메뉴를 항상 닫는다. t-0541의 상태 유지 단언을 대체한다. 1→2→3에서 메뉴를 열고 가장자리 쓸기 + touchcancel + 뒤로는 2에 메뉴 닫힘이며, 매번 다시 열어도 1→2→3의 양방향 이동에서 닫힌다. 닫힌 메뉴가 이동으로 열리지 않는지도 확인한다. WebKit 390·430 × Safari 탭·홈 화면 앱 흉내에서 popstate·선택 줄 aria-current·drawer transition을 기다리고 기록 칸 수를 확인한다. WebKit 1280·820에서는 기존 사이드바 펼침·접힘 유지 검사를 보존한다.
- E2E39의 `history-destinations.mjs`는 실제 host·agent의 thread 질문·게시 보고서·할 일을 준비하고 Chromium·WebKit 390px에서 thread·보고서 상세/목록·할 일 목록·질문 카드/미선택 목록·설정 목록/쪽을 메뉴 열림·닫힘 각각에서 뒤로·앞으로 순회한다. `history.back/forward`의 popstate 뒤 주소·본문 DOM·React frame·drawer transition으로 끝낸다. hash가 그대로인 설정 목록↔쪽·본문 복귀도 포함하며 hashchange만 처리하는 수리가 빠지는 경로를 검사한다. 빈 주소를 보던 pane으로 정규화하는 조기 반환과 목록 줄 직접 선택·빈 화면의 메뉴 단추도 확인한다. 할 일 상세는 같은 주소의 시트 기록이므로 별도 순회한다. 실제 앱 단축키로 메뉴를 연 뒤 Back의 상세 닫기·Forward의 닫힌 시트 건너뛰기와 메뉴 닫힘을 확인한다. 고정 sleep·시한 연장·실패 재시도는 없다.
- t-0636(plan §8.408) 쓸기 자리: 왼쪽 끝(x 4)에서 시작한 쓸기는 목록을 열지 않고 주소·기록 그대로, 안쪽(x 60) 오른쪽 쓸기는 목록을 연다. 가장자리 쓸기 + touchcancel + 뒤로와 쓸기 끝난 뒤 뒤로는 앞 pane에 목록 닫힘이다. 왼쪽 끝 24px 경계와 메뉴 단추·안쪽 쓸기는 바꾸지 않는다.
- 헤드리스는 Safari 시스템 몸짓 자체를 실행하지 못한다. 오른쪽 쓸기는 합성 touchstart·touchmove·touchcancel 뒤 history.back으로, 일반 뒤로·앞으로는 history.go로 같은 문서의 기록 순회를 흉내 낸다. 실제 iPhone Safari·홈 화면 앱의 운영체제 몸짓까지 대신 보증하지 않는다.

## 자동화 상세·변환·막힌 큐(t-0468, plan §8.303)

- `node web/e2e/web.e2e.mjs --spec 40-automation-detail`: 상세의 기록 수는 host의 `routine runs`와 같아질 때까지, 원문은 파일 글과 같아질 때까지 기다린다. 변환은 미리보기 sheet가 그린 묶음(`data-group`) 뒤에 파일이 그대로인지 보고, 실행 결과 표지(`convert-archived`) 뒤에 보존 사본을 원본과 비교한다.
- 자동화 화면(t-0628): 34·40·48·49·55·64는 카드를 눌러 본문 설정 화면을 열고, 닫기 단추 뒤 그 화면이 숨는 것(`hidden`, 숨긴 채 그려 둔다)을 기다린다. 떠 있는 창(`.modal-scrim`)이 없는 것과 저장·표지 뒤 주소(`#automation=<project>/<name>`)를 본다. 숨은 설정 화면 머리에도 목록 단추가 있어 휴대폰 목록 단추는 `:visible`로 고른다.
- 막힌 큐는 실제 전송 실패로 만든다: fake thread의 pane을 `pane kill`로 끝내고(목록에서 끝남 확인) 1초 일정의 `do.message` 규칙을 둔 뒤 `ticker tick`으로 host 기록에 「입력 전달 모름 · 확인 필요」가 올 때까지 판정한다. 「다시 보내지 않음」은 tick 두 번(울타리) 뒤 같은 묶음 id·실패 기록 수로 단언한다. 「계속 보내기」 뒤에는 pane이 끝난 채라 다음 입력이 실제로 시도되어 실패하고 새 묶음으로 다시 막히는 것을 `ticker tick`·host 기록으로 기다린다. 그 묶음은 「버리기」로 닫고, 규칙을 끈 뒤 tick 울타리 뒤 큐가 빈 것을 본다. fake agent의 전송 지연(재시작 뒤 첫 글이 0~15초)에 기대지 않는다. 고정 sleep은 없다.
- host 쪽 묶음 상태(보낸 결과 모름·다른 동작 묶음·버리기)는 `cargo test -p aaa-host --lib automation::tests`가 DB의 큐 JSON을 직접 두고 본다.

### 파일 오류와 실행 실패·배포 중 기능 대기(t-0560, plan §8.362)

- `node --test web/test/automationModel.test.mjs`: 정상 파일의 `start:config`·`flow:error` 등 마지막 `failed`는 「실행 실패」,
  깨진 파일의 `error`는 「파일 오류」다. 파싱 실패는 `enabled=false`로 와도 꺼짐에 가려지지 않는다.
- `cargo test -p aaa-host --lib automation::start_tests::rollout_`: 실제 host·DB·agent frame fixture에서 `reuse=open` 승인 범위가
  끊김·기능 부족에도 같고, 흐름 입력이 수집되어 원인과 「다시 시도 가능·입력 보존」을 알리는 대기로 남는지 본다.
  host 재시작·새 agent 재연결 뒤 동일 사건 처리·receipt 재생·시작 예산을 확인한다. 혼재한 후보 중 가능한 머신을 고르는 경계도 본다.
  시작 완료는 `thread_events`의 실제 Open 전이로 기다린다. thread 생성 영수증은 배경 기동 완료를 뜻하지 않는다.
- `node web/e2e/web.e2e.mjs --spec 64`: 정상 TOML의 없는 대상에 실제 실행을 요청해 host 실행 실패를 만든다.
  CLI와 Front API의 오류 칸·지난 결과·원인을 비교하고, 목록 표지·상세 칩·요약이 실행 실패와 TOML 파싱 실패를 구분하는지 검사한다.
  휴대폰 WebKit(390×844)에서도 구분하고 두 화면을 촬영한다. 고정 sleep·운영 설정·운영 DB 쓰기는 없다.

## 자동화 만들기·고치기·지우기·지금 실행(t-0538 관리 GUI W2, plan §8.342)

- `cargo test -p aaa-host --lib automation::edit`: 폼 값 합치기의 바이트 보존(바꾼 칸만, 주석·순서 그대로), 조건 바꾸기와 기본값 칸 빼기, 새 파일의 칸 순서와 `#:schema` 머리, 첫 칸이 사라져도 남는 머리 주석, inline 표·`do = "deploy"`, 잘못된 폼·원문의 거부.
- `cargo test -p aaa-host --lib automation::tests`: 실제 Host로 Reader 검사 허용·저장 거부, 새로 만들기 중복·본 판이 아닌 저장·잘못된 글이 폴더 전체 스냅샷을 바꾸지 않음, 지운 뒤 낡은 판 저장이 NotFound(되살아나지 않음), 되살리기는 꺼진 채 한 번, 옛 `.md` 저장 거부, 삭제의 대기 입력 취소·처리 중 묶음 보존, 지금 실행의 같은 key 한 번·일정 차례와 따로 받기·거부 넷, 본문 파일의 판 대조와 automations/ 밖 새 경로 거부, 명령 승인의 세션 안·worker·evaluator·detached·Reader 거부와 Controller 허용.
- `node web/e2e/web.e2e.mjs --spec 55-automation-edit`: 폼으로 만든 원문이 검사 응답(TOML 탭)과 파일에 같은지, 「저장만으로는 돌지 않음」을 thread 사건 규칙 저장 뒤 `ticker tick` 두 번(울타리) 뒤 기록 0건·안내 0건으로 본다. 충돌은 파일 직접 편집과 둘째 브라우저 저장 둘 다로 만들고, 거부 뒤 파일 글이 그대로인지·차이 줄을 본다. 지운 규칙은 a 편집기를 연 채 b가 지운 뒤 a 저장으로 되살아나지 않는다. 지금 실행은 inbox 항목·`manual:` 기록을 기다리고, 같은 key 둘째 호출 뒤 tick 울타리 뒤 안내가 늘지 않는 것을 본다. 시작 범위 미승인 거부는 실제 labA 머신이 있는 규칙으로 본다. 고정 sleep은 없다.
- 「한 초안」 단계는 TOML을 고친 뒤 debounce를 기다리지 않고 폼 탭에서 칸을 고쳐 저장하고, 파일에 두 쪽 고침과 머리 주석이 모두 남는지 본다(탭 전환이 다른 쪽 고침을 먼저 검사한다).
- 결함 심기: host 저장의 판 대조(사전 대조와 `file_rev` closure)를 빼면 `save_writes_only_the_seen_revision_and_never_resurrects_a_deleted_rule`이 「본 판이 아닌 저장」 단언에서 실패한다. 탭 전환의 먼저 검사를 빼면 E2E 55 「한 초안」이 실패한다(library/t-0538/w2-fault-replant).

## 운영 화면: thread 관리·「이 aaa」·저장소·알림함(t-0538 관리 GUI W4, plan §8.343)

- `cargo test -p aaa-host --lib project::tests::repo_edit_replaces_in_place_and_settings_view_reads_it`: 저장소 제자리 고치기의 차례·다른 칸 보존, 머신 빼기, 이미 있는 값·없는 값 거부, 설정 읽기의 파일 이름·숫자·저장소.
- `cargo test -p aaa-host --lib library_links::tests::prune_apply_deletes_only_the_previewed_candidates`: 미리 본 뒤 기한을 넘은 사본이 늘면(시각 주입) `conflict`로 하나도 지우지 않고, 같으면 본 것만 지운다.
- `cargo test -p aaa-host --lib db::flow_tests`: 흐름 사건 `tail`이 최근 N개를 오래된 것부터 준다.
- `cargo test -p aaa --test m13 resolve_keeps_dirty_worktree_and_unmerged_branch`: 실제 agent·git으로 `thread resolve --dry-run`이 미커밋 1·base 대비 2·병합 안 됨·남기기 권함을 보이고, thread 상태와 흐름 사건 수를 바꾸지 않는다.
- `node --test web/test/operateModel.test.mjs`: 동작 가능 판정(도는 세션·데려온 thread·준비 중·끝냄·정리 중·받은 brief), 끝내기 기본값과 요청, 저장소 인자·묶음, 숫자 범위, 알림함 배지와 「답할 것」으로 보낼 post.
- `node web/e2e/web.e2e.mjs --spec 56-operate`: 저장소 더하기·제자리 고치기(파일 글과 차례)·빼기 확인 창·할 일 저장소 목록, 「이 aaa」 값 저장(파일 글)과 범위 밖 숫자, 멈춤·재개 확인 창, 위험한 thread(미커밋·branch 커밋·사본에 빠진 symlink) 끝내기 창의 위험 줄과 남기기 켬, 끝낸 뒤 worktree·branch 남음, 깨끗한 thread(새 파일·big.bin을 치우고 사본이 다 온 뒤)의 남기기 끔과 worktree 지움, 다시 열기·Next 더하기·보내기·요약 고치기·세션을 끈 뒤 다시 시작, 알림함 단추·배지·패널 숨김과 CLI 조회·끝내기 보존(`seen` 무변경), 정리 미리보기, 휴대폰 390 폭, 같은 「바로 시작」 두 번 → thread 하나, Reader에 쓰기 단추 없음. 사본 상태·세션 상태·파일 글로 기다리며 고정 sleep은 없다.
- 결함 심기: library/t-0538/w4-fault-replant.

## host 계정 화면(t-0538 관리 GUI W5a, plan §8.351)

- `cargo test -p aaa-host --lib account_meta::`: Codex id_token의 메일·plan과 깨진 값, Claude 「valid for N」 만료와 `.claude.json` 메일(없으면 모름), 상태 판정(거부 > 만료 > 7일 안), 웹 조작 기기의 로그인·기본 허용과 보기 전용·세션 안·worker 거부, 보기 전용·세션 안 목록에서 메일·plan 가림, 끊김·곧 만료 알림 한 번(메일 없음)과 로그인으로 끝냄·계정 지우기로 정리, 바뀐 메일의 최초 시각.
- `cargo test -p aaa --test m13 accounts::`: 가짜 공식 CLI(`install_login_fixture`, `.claude.json`도 쓴다)로 로그인 → 메일·plan·날짜·사람이 읽는 목록, Claude 3일 → 곧 만료 알림, 같은 메일 다시 로그인은 최초 날짜 유지, 다른 메일 → `mismatch`(금고 그대로, 같은 별칭 로그인 거부) → `--keep`·`--switch`, 실험실 전체(host DB·fixture 밖)에 `@example.com`이 없음. 병렬 갱신 시험은 제공자가 거부한 갱신 뒤 계정이 `rejected`이고 알림함 `account` 항목이 하나인지 본다.
- `node --test web/test/accountsModel.test.mjs`: 1.79 판정, 별칭 꼴, 상태 낱말(남은 날 올림), 날짜, harness 묶음과 기본, 로그인 진행 글(host Mac 브라우저 안내·이 메일·다른 메일).
- `node web/e2e/web.e2e.mjs --spec 58-accounts`: PATH 앞 가짜 `claude`·`codex`가 FIFO에 쓸 때만 끝난다. 새 로그인 진행 → 메일·plan·날짜·정상, 기본으로·기본 해제(host 기본), 진행 중 취소(계정 없음), Claude 3일 → 곧 만료 배지와 알림함 항목(메일 없음), 다시 로그인의 「이 메일로 로그인하세요」 → 다른 메일 → 금고 그대로 → 「새 메일로 바꾸기」, Reader에 메일·로그인 단추 없음, 휴대폰 390 폭. 화면 표지(`data-state`)와 host 기록으로 기다리며 sleep은 없다.

## 웹 머신 등록·공개·보고서 올리기(t-0538 관리 GUI W5b, plan §8.355)

- `cargo test -p aaa-host --lib library_upload::`: 올리기 권한(Reader 거부)·경로(숨은 이름·`..`·절대·역슬래시)·올리기 이름·없는 프로젝트, 합계 64MiB(16MiB 넷은 되고 1바이트 더는 `too_large`, 같은 경로 다시 올리기는 그 크기를 빼고 셈), 사용자 보고서의 맨 위 index.html이 없으면 거부하고 올린 것을 둠 → 더 올린 뒤 등록하면 하위 폴더까지 보고서가 되고 올리기 폴더가 지워짐, 세션 안 agent·Reader의 등록 거부, 웹이 올린 수와 다르면(올린 자리가 없어도) `conflict`. `a_stale_upload_goes_away_but_the_one_being_written_stays`는 하루 넘은 올리기 둘(표지 `<올리기>.put`의 시각으로 만든다: 폴더 시각은 OS마다 달라 쓰지 않는다, plan §8.358) 가운데 이어 쓰는 것만 남고 그 표지 시각이 새로워지며 다음 올리기의 치우기도 그것을 남기는지, `an_upload_under_a_thread_numbered_state_path_has_no_origin_thread`는 상태 폴더 경로의 thread 번호를 근원으로 읽지 않는지 본다.
- `cargo test -p aaa-host --lib pairing_tests::the_web_keeps_its_last_controller_and_the_host_machine`: 웹에서 host 머신 폐기 거부, 조작 기기가 하나일 때 그 기기 폐기 거부(host CLI는 됨), 둘이면 웹 폐기 허용. `trust_management_requires_a_person_outside_the_session`은 조작 기기가 등록 창을 열고 Reader는 거부하는지 본다.
- `node --test web/test/adminModel.test.mjs`: 1.80 판정, 대소문자·NFC/NFD만 다른 이름 막기, 확인 코드(공백·하이픈), 등록 창 남은 분, 마지막 조작 기기(보기 기기는 세지 않음), 공개 주소 판정과 다른 경로(tailnet 이름의 http 주소는 내지 않음), 보고서 이름(host `name_ok`와 같음), 폴더 올리기 경로(맨 위 폴더 빼기·숨은 파일 빼기)와 상한 까닭, 살아 있는 링크, 올리기 이름.
- `node web/e2e/web.e2e.mjs --spec 60-admin`: PATH 앞 가짜 `tailscale`(serve 상태는 실험실 파일, 실제 tailscale을 부르지 않는다)과 같은 Mac에서 다른 설정·상태 폴더로 띄운 둘째 agent(host `agent_listen`을 loopback에 연다). 등록 창 열기 → `aaa agent enroll`이 찍은 확인 코드 → 대기 목록(enroll 사건) → 틀린 코드의 말 → 맞는 코드로 승인 → enroll 끝 코드 0 → 연결(state 사건) → 창 닫기, host 머신에 폐기 단추 없음, 폐기 확인 창 → 목록·host에서 빠짐, 마지막 조작 기기의 폐기 단추 꺼짐, 공개 켜기(확인 창)와 loopback 화면의 끄기(다른 경로만), 공개 주소로 연 화면(실험실은 TLS 공개 주소를 열 수 없어 `web_status` 답의 주소만 이 화면 origin으로 바꾼다)의 「이 화면이 끊길 수 있음」과 확인 전 끄기 꺼짐, 폴더 올리기(맨 위 폴더·숨은 파일 빠짐, 사용자 보고서·제목, 올리기 폴더 지움)와 여러 파일(index.html 없는 사용자 보고서 막음 → 작업 보고), 키 교체 확인 창의 살아 있는 링크 수와 옛 링크가 열리지 않음, Reader에 등록·폐기·공개·올리기·키 교체 없음, 휴대폰 390 폭과 이 기기 폐기 확인 → pairing 화면. 화면 표지·host 기록·프로세스 출력과 끝으로 기다리며 sleep은 없다.
- 이 단계 ci-local에서 E2E 53(t-0540)의 「알림함 배지가 답할 요청을 세지 않음」이 5 → 6으로 실패했다. 실험실 DB를 보니 앞 단계 보고의 「report needs user」 항목이 요청 post 16ms 전에 들어와 앞뒤 배지 대조 사이에 끼었다. 대조를 「배지 = host의 끝내지 않은 항목 − 요청 1」로 바꿨다(늦게 온 항목은 둘을 함께 바꾼다). 배지가 요청을 세게 결함을 심으면 이 단계가 실패한다.

## 웹 profile·safety·작업 폴더·hook(t-0538 관리 GUI W5c, plan §8.357)

- `cargo test -p aaa-host --lib settings_admin::`: 웹이 바꾸는 safety key는 넷뿐(`*_agent_args`·`routine_commands`는 `forbidden`, 파일 그대로), 프로젝트 표·모든 프로젝트 표(`[safety.default]`)에 쓰고 「기본」은 그 key를 지움, 앞 판(`expected`)으로 보내면 `conflict`이고 파일 그대로, 주석 보존. custom profile 만들기·고치기·harness를 custom으로·지우기 거부, 목록 JSON에 env 값과 custom 실행 명령이 없음, 미리보기는 쓰지 않고 적용 뒤 실행 인자가 미리보기와 같음, 낡은 판 적용은 `conflict`. 허용 목록·기본값은 없는 이름 거부, 적용 뒤 host의 thread 허용·coordinator 기본이 바뀜. 세션 안 agent·worker·detached의 바꾸기 다섯 가지 거부와 Reader의 읽기 거부. 작업 root는 `~`·상대·`C:work`를 agent에 닿기 전 거부. `profiles_from_host_local_toml_stay_out_of_web_edits`는 host.local.toml의 custom 이름으로 더하기·local이 effort를 덮는 profile을 shell로 고치기·지우기를 거부하고 파일과 설정 읽기가 그대로인지, `global_edits_warn_when_the_project_table_wins_and_rows_tell_the_value_below`는 프로젝트 표가 이기는 모든 프로젝트 기본 고치기의 경고와 「기본」으로 yolo가 켜지는 `below`(host.local.toml이 같은 key를 정하면 그대로)를, `toml_errors_do_not_carry_the_broken_line`은 깨진 줄·형식(타입) 오류의 env 값이 오류 글에 없고 줄 첫 칸 오류도 그 줄 번호인지, `a_host_without_a_project_still_edits_global_safety_and_profiles`는 프로젝트 없는 host를 본다.
- `cargo test -p aaa-proto control_classification`: 설정 관리 읽기도 조작 권한.
- `node --test web/test/settingsModel.test.mjs`: 1.81 판정, yolo 켜기만 한 번 더 확인(아래 층이 on인 「기본」 포함), 고치기는 고친 칸만, 인자 줄마다 하나(빈칸 든 인자), 기본 profile 고치기는 같은 이름 더하기, harness를 바꾸면 받지 않는 model·effort 비움, profile 이름(host `validate_name`과 같음), 절대 경로(host `absolute`와 같음), 전환은 점검한 그 경로·mount일 때만.
- `node web/e2e/web.e2e.mjs --spec 61-settings-admin`: 실험실 `CODEX_HOME`(사람이 둔 다른 hook 하나). 「안전」에서 넷만 바꾸는 칸(인자·명령 목록은 「host 터미널에서」), start_threads auto → `aaa safety show` 값·출처, yolo 켜기 확인 창 → host.toml, 실험실 밖에서 host.toml을 고친 뒤 바꾸면 「그사이」 알림이고 파일 그대로 → 다시 바꾸면 사람이 고친 줄을 두고 씀. 「profile」의 custom `fake`에 단추·실행 명령 없음, 더하기 창(harness에 custom 없음) → 미리보기의 실행 인자(빈칸 든 인자 포함) → 미리보기만으로는 파일 그대로 → 적용 → `aaa profile resolve`. 허용 목록(실험실 기본 fake → 이 프로젝트는 e2e-opus만) → `aaa safety show`. 「머신」 작업 폴더: 상대 경로는 점검 꺼짐, 점검 전 전환 꺼짐, 「검사 파일을 만듭니다」 확인 → 없는 폴더를 만들고 통과, 점검 뒤 경로를 바꾸면 전환 꺼짐, 전환 → `agent.local.toml`에 새 경로·재시작 필요 표시, host.toml 그대로. Codex hook 설치 → 설치됨·aaa hook 세 사건·다른 hook 그대로 → 제거 → 없음·다른 hook 그대로. Reader에 안전·작업 폴더 없음, 휴대폰 390 가로 넘침 없음. 화면 표지·파일·host 답으로 끝내며 sleep은 없다.

## 웹 배포·재시작(t-0538 관리 GUI W6a, plan §8.361)

- `cargo test -p aaa-host --lib rollout::web_tests`: 웹 조작 기기의 `deploy_start`는 재시작만(교체는 `forbidden`), 재시작은 권한을 지나 `deploy:<id>`에 프로젝트·`web`·시각을 남기고 같은 ID 재전송은 처음 시각 그대로·다른 작업이면 `bad_request`. 세션 안 coordinator·worker·Reader의 `rollout_plan` 거부, 조작 기기는 상대 경로 `bad_request` → 끊긴 빌드 머신 `agent_disconnected`, 웹 rollout에 빌드 머신 없는 후보는 `bad_request`. 사람·웹 기기 actor는 coordinator_deploy가 꺼져도 승인 유지, coordinator·빈 `device:`는 아님. `rollouts`는 이 프로젝트만 새것부터, 조작 기기의 같은 ID 조회는 요약(actor·후보 경로·오류 원문·작업 ID 없음), Reader·다른 프로젝트 거부. 진행 중 rollout이 있으면 점검 거부, 같은 ID 재전송은 빌드 머신이 끊겨 있어도 기존 요약. `deploy_jobs`는 새것부터·옛 기록(시각 없음)은 맨 뒤·다른 프로젝트 빠짐·끊긴 머신은 상태 없음·coordinator 교체 작업은 종류만(후보 경로 없음), 작업 끝 `deployment_changed`가 `rollout` 사건을 보냄, 실패 영수증은 고정 까닭만(PID·결과 원문 없음). 끊긴 머신(기능만 남음)이라 agent에 닿기 전에 끝나며 시간을 기다리지 않는다. 시험 몸은 따로 만든 runtime에서 돌리고, runtime을 끝내 `agent_connected`가 띄운 task(한 번도 돌지 않은 `rewatch`)가 쥔 Host가 닫혔는지 본 뒤 상태 폴더를 지운다(Windows는 열린 host DB가 든 폴더를 지우지 못한다, os error 32).
- `cargo test -p aaa-agent --lib candidate_inspection`: 대기 중인 배포 작업이 있으면 점검 거부(거절 글에 작업 ID 없음, 실행기 staging 이름이 겹친다)와 점검 중에 들어온 작업 거부. 후보 읽기가 provenance의 SHA·커밋·빌드 시각과 dry-run 판을 답하고, 판의 커밋 표시가 다르거나 커밋이 40자리가 아니면 거부, 실행기 실패는 고정 글(경로·후보 출력 없음), provenance와 다른 파일은 실행하지 않고 거부, `aaa`가 아닌 이름 거부. 가짜 실행기를 쓴다.
- `node --test web/test/deployModel.test.mjs`: 1.82 판정, ID가 host `valid_id`(영숫자 26자), 후보 경로 형식, 대상은 host 맨 앞(다른 머신을 모두 풀면 host만), 확정 거절일 때만 보인 ID를 지움, 시작은 점검한 그 후보·대상이 모두 준비되고 진행 중 rollout이 없을 때만, 보낼 rollout의 SHA·커밋은 점검 결과 그대로.
- `node web/e2e/web.e2e.mjs --spec 63-deploy`: 실험실 집(`HOME`)과 PATH 앞 가짜 `launchctl`(재시작이 이 Mac의 실제 aaa 서비스에 닿지 않는다, 감독자 plist도 실험실 집). 후보는 허용 표지가 있을 때만 self-check를 통과하는 셸 파일과 provenance.json이다. provenance SHA가 다르면 점검 오류·시작 꺼짐 → 맞으면 SHA 전체·판·대상 「labA (host 먼저) · 준비됨」·시작 열림, 점검 뒤 경로를 바꾸면 시작 꺼짐. 표지를 지운 뒤 확인 창(SHA) → rollout ID 표시 → 기록에 `stopped`(대상 전송의 dry-run에서 멈춤), 실험실 실행 파일 SHA 그대로·launchctl 호출 없음. 화면을 다시 열면 같은 ID의 카드(「배포 중단」, 끝난 단계는 전송뿐, 후보 경로 없음). agent 재시작 확인 창 → 작업 「완료」·가짜 launchctl의 `kickstart -k gui/<uid>/dev.aaa.agent`. Reader는 기록·작업만 보고 시작·재시작 단추 없음, 휴대폰 390 가로 넘침 없음. 화면 표지·파일·host 사건으로 끝내며 sleep은 없다.

## 안전 모드(t-0538 관리 GUI W6b, plan §8.370)

- `cargo test -p aaa-host --lib safe_mode::`: 원문 가림(env 표·인라인 env·비밀처럼 보이는 key·표 머리의 비밀 낱말·webhook·그런 낱말이 든 값·주석으로 꺼 둔 줄, 줄 수 그대로, 다른 한 줄 값과 표 머리는 남음, 가린 값은 오류 글에서 뺄 목록에). 여러 줄 값의 이어지는 줄은 모두 가림: 닫히지 않은 문자열은 끝까지, 줄마다 하나씩 쓴 인자·배열 안 `["…"]`, 배열이 닫힌 뒤는 다시 보임, `]`를 빠뜨린 배열은 뒤의 표 머리(공백 없이 주석이 붙어도)에서 끝나 그 env 값을 가림, 주석으로 꺼 둔 비밀 배열·여러 줄 문자열은 이어지는 주석 줄을 가리고(사이 빈 줄은 건넘) 주석이 끝난 바로 뒤 값·표는 보임, 비밀이 아닌 배열의 원소는 화면에서만 가리고 오류 글에서 지울 목록에 넣지 않음(닫히지 않은 문자열 안에 표 머리가 나온 뒤의 값은 넣음). 마지막 정상 설정은 설정 폴더 `last-good/`에 둔다(상태 폴더에 env·인증 값이 남지 않는 것은 m13 `secret_k0`·`accounts`가 본다). 오류 글은 TOML 원문 조각(`|` 줄)과 큰따옴표 값·가린 값을 뺌. 정상 설정 보관(폴더 0700·파일 0600, 시작 뒤 깨진 파일은 보관하지 않고 지난 보관본 유지)과 되돌리기(지금 파일을 `.broken-<UTC>`로 남김, 보관 때 없던 local은 남긴 뒤 치움, 같은 이름의 남긴 파일을 덮지 않고 그때는 먼저 만든 남기기도 치우고 아무것도 바꾸지 않음, 바뀐 게 없으면 남기지 않음, 보관이 없으면 거부). 뜰 주소는 마지막으로 연 주소(실제 `web::start`가 적음) → 보관 설정의 listen, 둘 다 없으면(업그레이드 직후) 짐작하지 않음, 웹을 끈 설정으로 시작하면 없음. 조작 기기만 오류 줄 표시·가린 원문·되돌리기(Reader·짝 없음·모르는 token은 안내만), API는 503 `unavailable`, 다른 Host 거부. 되돌리기·다시 읽기는 조작 기기와 이 origin만(다른 Origin·Origin 없음·Reader·짝 없음은 파일을 건드리지 않음), 아직 깨져 있으면 안전 모드에 남고, 되돌리면 남긴 파일·감사 기록과 함께 끝을 알린다. 실험실 DB와 router를 바로 부르며 시간을 기다리지 않는다.
- `node web/e2e/web.e2e.mjs --spec 68-safe-mode`: Reader·휴대폰 기기를 먼저 짝짓고 host를 한 번 다시 띄워 고정 port 설정이 보관되는 것을 본다. host.toml 끝에 env 값과 닫히지 않은 표를 붙이고 다시 띄우면 같은 주소의 `/api/v1/me`가 503 `unavailable`(「안전 모드」) → 조작 기기 화면에 `host.toml:<줄>:` 오류·그 줄 표시·`E2E_LABEL = «가림»`, 페이지에 env 값 없음. Reader는 안내만(원문·되돌리기 없음), 휴대폰 390 가로 넘침 없음. 「마지막 정상 설정으로 되돌리기」 → 안내 화면, host.toml이 보관 설정 그대로·깨진 파일이 `host.toml.broken-*` 하나로 남음 → 안내 화면이 스스로 다시 열어 정상 앱(t1 pane)이 뜨고 labA가 다시 붙으며, 안전 모드를 연 host 프로세스가 그대로 정상 host가 된다. 파일·HTTP 답·화면 표지로 끝내며 sleep은 없다.

## 대화의 도구 호출 묶음(t-0477, plan §8.304)

- `node --test web/test/toolGroups.test.mjs`: Claude·Codex 도구 이름 분류, 요약 문구(두 가지까지·우선순위·서로 다른 수정 파일·생각), 이어진 도구·생각 줄 묶기, 앞 쪽이 묶음 앞에 붙은 뒤에도 옛 첫 줄 id로 같은 묶음과 펼침을 찾는지 본다.
- `node web/e2e/web.e2e.mjs --spec 41-chat-tool-groups`: 실제 lab host/agent/Claude JSONL에서 데스크톱 Chromium 1280·패드 WebKit 820·휴대폰 WebKit 390을 열고 묶음 요약·접힌 동안 보이는 도구 줄·펼친 줄을 단언한다. 끝이 아닌 곳에서 펼치고 접은 뒤 머리 좌표가 1px 안에서 같고, 끝에서 펼친 뒤에도 끝인지 DOM·animation frame 사건으로 본다. 스크린샷 폴더를 주면 기기마다 끝·첫 묶음·펼침을 찍는다(전후 캡처). 고치기 전 코드로는 묶음 요약이 비어 실패했다. 둘째 단계는 실험실 `claude`(harness `claudeSh`)가 SessionStart hook으로 기록을 알리고 `pane run` 한 줄마다 작업 화면·입력 상자를 바꾸며, 사이드바 `data-status` 사건을 울타리로 맨 끝 묶음의 글과 `is-live`를 본다. 작업 중 판정을 끄면 「작업 중 마지막 도구 줄」에서 실패한다.

## 웹 할 일 보기(t-0480, plan §8.306)

- `cargo test -p aaa-host --lib tasks::`: 운영에서 나온 오독 꼴 셋(같은 꼴의 가짜 글)이 owner가 아니라 `problems`로 나오고 기존 `lines_owners_notes` 기대가 그대로인지, 메모만 바뀌면 그 task의 `block_sha256`만 바뀌는지 본다. 오독 판정을 끄면 `misread_owner_shapes_become_problems`가 실패한다.
- `cargo test -p aaa --test m13 task_board_reads_tasks_and_follows_file_edits`: `aaa task list`의 목록·형식 어긋남·task 없는 열린 thread, worker 세션(보기 권한) 읽기, `--from-task` 거부 까닭, 「나중」 경로 거부·폴더 밖 symlink·없는 파일. 「고치지 않은 tick은 사건이 없다」는 sleep 없이 본다: 원본 목록 읽기 → 사건 연결 → 초기 파일 쓰기 → tick 뒤 초기 sha 사건의 위치를 기억한다. 그 위치 다음에서 고치지 않은 tick → 파일 고침 → tick 뒤 첫 `tasks` 사건이 고친 sha인지 확인한다. 초기 사건을 다시 읽는 경쟁을 재현하며, 수정 사건을 sha로 걸러 기다리지 않아 불필요한 사건도 잡는다.
- `node web/e2e/web.e2e.mjs --spec 43-tasks`: 막대 단추로 열기, 행 상태·배지·상세·thread 칩 이동, 파일 직접 편집 + `ticker tick` 뒤 `data-tasks-sha` 갱신, 거르기·검색, Reader, 휴대폰 전체 화면 시트와 Back, 패드 세로 시트·가로 두 칸, 「나중」 탭 있음·없음·「없음」. 가짜 thread가 곧 턴을 끝내 앱 안 알림이 도구줄을 덮으면 Playwright가 알림이 시간으로 사라지기를 기다리므로(6초), 첫 단계에서 그 기기의 「앱 안에서」 알림을 끈다.

## 웹 할 일 편집(t-0482, plan §8.311)

- `cargo test -p aaa-host --lib task_edit::`: 순수 함수 `apply`. 바꾼 블록 밖 바이트(CRLF 포함) 보존, 대상 블록만 충돌(다른 task 변경은 통과, 지운 task 고치기는 충돌), 저장 검사(`)` 끝 제목·`· t-` 끝·task 꼴 메모·중복 제목), 하위 묶음째 지우기·옮기기, 목록 동작, 되살리기, 파일 직접 차이 기록, 한 요청 안 고치고 옮기기(hash 따라가기), 원문 저장 홀로. 대상을 블록 hash 대신 제목으로 찾게 하면 `block_anchor_conflicts_only_on_the_target`·`update_then_move_in_one_request`가 실패한다.
- `cargo test -p aaa-host --lib file_rev::`·`tasks::tests::suggestions_for_misread_lines`: 공용 저장 helper(판 대조·다시 읽기), 정리 제안 후보.
- `cargo test -p aaa --test m13 task_edit_keeps_bytes_conflicts_and_logs`: CLI 더하기의 바이트와 `tasks` 사건, `--block` 충돌, 지운 task 고치기 충돌, 쓰기에 묻힐 뻔한 직접 편집 기록, 되살리기 한 번, 저장 검사, 다음 tick의 「파일 직접」 기록, 권한(worker 세션 보기 권한 거부, 확인되지 않은 셸·확인된 사람 셸 pane은 「그 프로젝트 coordinator만」, coordinator pane 허용). 확인된 pane을 허용하게 느슨히 하면 실패한다.
- `cargo test --profile verify --locked -p aaa-host --lib lifecycle::task_memory_tests`: 지운 프로젝트를 같은 이름으로 다시 만들면 첫 tick은 기억만 하고 「파일 직접」 기록을 지어내지 않는다(지울 때 기억을 남기면 실패).
- `node --test web/test/tasksModel.test.mjs`: 메뉴·끌기 → host 동작, 폼 → 동작(상위는 고른 판의 블록 고정점, owner 목록 전 폼은 동작 없음), 「나중」 항목, 기록 차이.
- `node web/e2e/web.e2e.mjs --spec 46-task-edit`: 폼으로 더하기·고치고 옮기기(파일 글 전체를 기대값과 바이트로 비교), 사람 이름 owner 거부, 정리 제안, 행 메뉴·끌기·Alt+↓·목록 이름 바꾸기·빈 목록 지우기·새 목록, 두 브라우저 충돌 창과 「최신으로 다시 열기」, 폼을 고치는 동안 다른 task가 파일에서 바뀌어도 저장됨(대상만 충돌), 지우기·되살리기 줄·기록 거르기·기록에서 되살리기, 저장만으로 thread가 시작되지 않음(`ticker tick` 울타리 뒤 흐름 사건·thread 수), 보기 권한 기기에 편집 UI 없음(host 판 판정이 끝난 `data-can-edit="false"` 뒤에 본다. 판정 전은 `pending`), 명령 팔레트 「할 일 더하기」(할 일 화면을 연 적 없는 기기), 휴대폰 「+」·전체 화면 폼·메뉴 시트, 패드 세로 오른쪽 시트(상자 좌표로 단언), 「나중」 할 일로 옮기기(「나중」 파일 그대로), 원문 편집의 판 대조. 저장이 끝났는지는 시간이 아니라 화면 `data-tasks-sha`가 새 판이 되고 `data-task-saving`이 없어진 뒤 그 판이 파일 sha와 같은지로 본다.
- 같은 E2E 46의 「터치 입력 글자」는 Chromium·WebKit의 iPhone 세로/가로에서 검색·제목·메모·선택·새 목록·사람 이름·첨부 붙여넣기·원문·목록 이름의 실제 계산 글자가 16px 이상인지 본다(t-0530). 폼의 13px 덮어쓰기를 복원하면 실패한다. headless는 iOS 키보드의 초점 확대를 재현하지 않으므로 실제 iPhone에서 입력·키보드 내림 뒤 배율 유지 확인은 별도다.
- 결함 심기: 웹 폼이 편집을 시작한 판 대신 최신 판 hash로 저장하게 하면 E2E 46 「충돌」 단계가, 팔레트 요청 수를 mount 때 값으로 두면 「명령 팔레트」 단계가 실패한다. 웹 결함은 `npm run build`가 dist를 다시 만든 뒤 `cargo build -p aaa`로 binary에 넣어 돈다.

## 일을 맡기기 전 task 찾기(t-0539, plan §8.336)

- `cargo test -p aaa-host --lib tasks::tests::start_warning`: 제목의 대소문자·구두점 정규화, 낱말 겹침 경계, 첫 열린 후보 선택, 완료·빈 제목·약한 겹침 제외와 제안 명령의 셸 인용을 확인한다. 형태소·동의어는 추측하지 않는다. 완료 task 제외를 제거하면 음성 시험이 실패한다.
- m13 `thread_start_warns_about_similar_tasks_without_blocking`: 실제 CLI 응답에서 경고 한 줄과 시작 성공, `--from-task` 시작·완료 task에 경고 없음, 기존 task 보존과 직접 시작의 별도 할 일 생성·자동 연결을 확인한다. 명령 응답이 끝이라는 신호이며 고정 sleep은 없다.
- 기존 m13 `doctor_files_brings_the_coordinator_guide_to_existing_projects`의 안내 문구 목록이 새 프로젝트 생성과 낡은 안내 갱신 모두에서 사전 검색·기존 thread 재사용·메모/모델/머신/저장소 갱신·`--link-task` 안내를 검사한다.

## 웹 할 일 thread 연결과 자동 정리(t-0493, plan §8.316)

- `cargo test -p aaa-host --lib task_clean::`: P2 판정표(병합은 표지와 무관하게 완료, `[완료]`만 완료, 그 밖 표지·보고 없음·사본 없이 끝냄·하위 남음은 확인 필요)와 확인 필요 까닭.
- `cargo test -p aaa --test m13 task_thread_link_and_auto_clean`: `--link-task` 줄 끝 번호, 열린 동안 판정 없음, `[완료]` 지움과 「host 자동 정리」 기록, 보고 없음·`[결정 필요]` 확인 필요(`task list`·`context`·inbox), 상위 올림, 되살린 task를 다음 tick이 다시 지우지 않음(한 번 판정 거르기를 없애면 실패). 판정은 `ticker tick` 울타리 뒤에 본다.
- `cargo test -p aaa --test m13 merged_thread_cleans_its_task`: 가짜 gh로 병합한 thread는 보고가 `[착지]`여도 그 task를 지운다(병합 판정을 끄면 실패). 병합 끝내기와 판정은 같은 tick에서 돌아 `thread_resolved` 사건 뒤 `ticker tick` 하나가 울타리다.
- `node web/e2e/web.e2e.mjs --spec 47-task-thread`: 바로 시작 확인창(`data-start-state`)과 줄 끝 연결, 확인창을 연 뒤 블록이 바뀌면 시작 거부, 자동 정리 줄·되살리기, 확인 필요 단추, 상위 올림, task 없는 thread 잇기·만들기, coordinator 맡기기(입력칸 초안을 localStorage로 확인), 휴대폰·패드 한도 확인.

`task_clean::tests::parent_attention_is_published_after_the_verdict_is_saved`는 실제 thread 기록 잠금으로 마지막
판정 저장만 보류한다. 하위 삭제·상위 연결의 파일 사건을 받고 같은 hash의 board를 읽으면 아직 확인 필요가 없다.
잠금을 풀어 판정을 저장한 뒤에는 같은 파일 hash의 `tasks` 사건이 다시 와야 하고, 그 board의 상위는 확인 필요다.
판정 저장 뒤 사건을 빼면 사건 누락 단언에서 실패한다. 시간 지연 주입·sleep·화면 재시도는 쓰지 않는다.

## 파일 한도 시험 실패 진단(t-0573)

`cargo test -p aaa --test resource_limits --locked`는 자식 host의 한도만 낮춘다. hard64 시험의 Front EMFILE
관측이 실패하면 그 실행의 host 로그, 단발 Front 요청의 stdout·stderr, 실제 host fd 목록과 두 PID의 상태를 남긴다.
Front 요청이 이미 성공했는지, 연결 전에 실패했는지, 여전히 기다리는지 구분할 근거이며 원인을 미리 단정하지 않는다.
원래 15초 상한과 EMFILE·복구·경고 한 건 단언을 유지한다. 실패한 실험실 폴더는 기존 Lab 정책대로 보존한다.

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

## 공개 검사 파일명·크기 경계(t-0483 후속)

- `python3 -m unittest scripts.test_check_public`: 실제 check-public CLI를 임시 Git 저장소에서 실행한다. 1MiB 경계·초과, 공백·개행·마지막 파일의 금지 확장자, 현재 트리에서는 지워졌지만 이력에는 남은 금지 확장자를 검사한다. 정상 현재/이력 입력도 함께 통과해야 한다. 모든 키 fixture는 빈 파일이다.
- 크기 예외는 정확한 경로와 상한을 지킨다. plan 2MiB와 버전 고정 Bold·Regular의 기존 바이트 상한까지 허용하고, 각각 1바이트 초과 및 다른 문서·폰트는 차단한다. 해당 세 파일은 삭제·축소하지 않는다.
- 기존 NUL 변수 경유로 돌아가면 세 keeper가 실패하고, 예외 상한을 없애면 크기 keeper가 실패해야 한다. 바이트 복원 뒤 ci-local 전체·check-public을 실행한다. ci-local의 Unix test 단계가 기존 배포 빌드 계약 시험과 함께 실행한다.

## 대화 정리(compaction) 중 감지·보류·표시(t-0479, plan §8.309)

- agent 감지는 실제 화면 fixture(`crates/aaa-agent/src/detect/fixtures/*compact*.txt`, 10-20 Claude 2.1.304·Codex 0.163 `/compact` 실측)로 본다: `cargo test -p aaa-agent --lib detect`. 「(0s)」 Claude 화면은 참고 규칙으로 쉼이라 덧댐이 없으면 시험이 실패한다(결함 심기로 확인).
- `node web/e2e/web.e2e.mjs --spec 45-compacting`: 가짜 Claude가 F7(작업 중)·F5(정리 시작)·F6(정리 끝)을 `aaa pane send-keys`로 받아 실제 정리 화면(spinner 「Compacting conversation… (3s)」, 참고 규칙으로는 쉼)과 실제 기록 차례(compact_boundary → isCompactSummary → 대기열 dequeue)를 그린다. 상태 전이는 가짜가 그린 「mode …」를 `waitOutput`으로, 화면은 `domIs`로 기다린다. 「보내지 않았다」는 보관 직후 `sentFrames`(그 사건 안에서 끝나는 보관)와 pane이 받은 bytes로 본다. 휴대폰·패드는 WebKit `pairDevice`. 같은 브라우저 탭 둘은 같은 context의 page 둘이다: 「한 번만 갔다」는 그 글을 품은 사람 말이 꼭 하나인지(두 탭이 함께 붙여 넣으면 한 말로 붙는다)와, 다른 탭이 다음 글을 보낸 뒤에도 그대로인지(입력창은 앞 글이 보내질 때까지 잠긴다)로 본다. 고정 sleep은 없다(넘겨받기 전 다른 탭에 묻는 500ms는 제품의 기다림이고, 시험은 기록에 오는 것을 기다린다).
- 웹 단위 `node --test web/test/messageQueue.test.mjs`: 정리 뒤 보낼 글의 저장 왕복(넣은 탭 `by`), 표지 떼기·넘겨받기, 정리 뒤 차례(`compactionStep`: 넣은 탭만 보냄, 앞의 보통 글을 앞지르지 않음, 자동 전송을 껐으면 표지를 뗌).
- 결함 심기로 확인한 것: agent 덧댐 끔(detect 시험 둘 실패), 옛 host에 compacting 그대로 보냄(`compaction_goes_to_an_old_host_as_working` 실패), host 표지 안 씀, agent가 청하지 않아도 정리 줄을 냄, 웹 보류 없음·「지금 보내기」 정리 중 켬·정리 중 줄 없음·정리됨 구분선 없음·끝나도 안 보냄·탭 주인 무시(「두 탭 하나두 탭 하나」)·넘겨받기 없음(각각 E2E 45 실패). Codex 답 글 오탐·긴 초안에 밀린 표지는 detect 시험이 고치기 전에 실패했다. 웹 결함은 `npm run build`(tsc 포함)가 실패하면 dist가 그대로라 옛 번들로 돈다: 심은 뒤 binary의 `index-*.js` 이름이 바뀌었는지 먼저 본다.


## PR 추적 gh 실행·인증·회복(t-0487)

PR 기능 폐지(t-1203)로 해당 owner를 제거하거나 일정 규칙 fixture로 전환했다. 현재 회귀는 아래 t-1203 절을 따른다.

## aaa install I0 계약(t-0485, plan §8.312)

- `cargo test -p aaa-config --lib install::tests`: 질문과 자동화 DTO의 같은 순수 검증기·고정 계획/hash,
  role별 필수/상충·OS 서비스·한국어 Windows 경로·출력/비밀 입력 모드·허용 env와 오류의 값 비노출을 검사한다.
  파일·HOME·프로세스·포트·OS 설정을 사용하지 않는다. I1 이후의 실제 탐지/CLI/설치 증거와 구분한다.
- `cargo test -p aaa-proto --lib install::tests`: 공개/비공개 fixture의 typed 왕복, 모르는 key/schema 거부,
  status/exit와 생성 schema drift를 검사한다. TS drift는 기존 `typescript_bindings_are_current`가 소유한다.
- `docs/install/agent-input.json` → `agent-plan.json`/`agent-plan.sha256`은 공개된 canonical JSON/SHA-256 계약의
  독립 fixture다. 해시는 compact JSON의 key를 재귀 정렬하고 UTF-8/BOM·끝 LF 없이 계산한다.
  테스트 실행 중 owner가 기대 hash를 갱신하지 않는다.
- SSH arch는 문자열 fixture `x86_64`의 역직렬화/직렬화를 검사한다. enum의 kebab-case 자동 변환으로
  `x86-64`가 되던 실제 계약 결함은 이 fixture가 수정 전 RED, 명시 snake_case 수정 뒤 GREEN이다.
- 새 보안/hash 검사는 허용 env 거부 제거, 대상 context를 hash에서 제외, 입력의 모르는 필드 허용을 각각 심어
  해당 owner 시험의 단언 실패를 본다. 각 변형은 원본 bytes로 복원한 뒤 다음 시험/전수 gate를 실행한다.
- I0는 승인서/등록 허가의 schema만 만든다. 서명·pin·nonce·W0 executor·일회 grant·실제 감독자/SSH 수명은
  후속 단계의 구현과 시험 전까지 통과했다고 표시하지 않는다. 가짜 fixture 서명은 승인으로 쓸 수 없다.

## PR 추적의 선택 실행과 CI 장애 분리(t-0492, plan §8.313)

PR 기능 폐지(t-1203)로 해당 owner를 제거하거나 일정 규칙 fixture로 전환했다. 현재 회귀는 아래 t-1203 절을 따른다.

## branch 변경 뒤 agent 재시작과 준비 기록 격리(t-0495, plan §8.315)

- `cargo test -p aaa --test m13 agent_restart_preserves_threads_after_branch_changes_and_isolates_invalid_preparations`: macOS 실제 host·agent·Git worktree 실험실에서 현재 볼륨을 명시한 휴대용 root를 쓴다. 준비 뒤 branch 변경·dirty 상태와 정상 thread를 함께 두고 agent만 재시작한다. 연결 상태·pane 읽기·기존 session host PID·Git 잠금으로 확인하며 고정 sleep은 없다.
- 별도 thread의 `.git`을 다른 repo로 바꾸되 원래 repo의 worktree 등록은 남긴다. JSON이 깨진 기록도 함께 둔다. 두 기록은 원인과 파일명이 경고에 남고 journal bytes는 보존된다. 정상 두 worktree만 aaa 소유 잠금을 복구해야 한다.
- 수정 전에는 agent가 repo·branch 불일치 오류로 종료했다. branch 거부를 다시 심으면 변경한 thread의 잠금 단언, 실제 repo 대조를 빼면 다른 repo의 잠금 금지 단언이 실패해야 한다. journal의 repo 칸만 바꾸는 입력은 Git 등록 검사도 거부하므로 그것만으로 repo 대조 제거를 검증했다고 하지 않는다.
- 기존 `work_root::recovery_tests`와 m13 `configured_prepare_timeout_recovers_only_unchanged_worktree`는 준비 재진입·dirty 거부·정리 잠금 계약을 계속 소유한다. 전체 ci-local·check-public 뒤 착지 창을 받는다.

## aaa install I1 읽기 CLI(t-0485, plan §8.317)

- `cargo test -p aaa --test install --locked`: 실제 binary/argv와 임시 HOME/config/state/root/repo를 소유한다.
  문법·전역 instance·필수값 오류의 정제된 JSON/exit, JSON/NDJSON의 같은 계획 hash, dry-run/오류 전후 파일 snapshot 불변을 본다.
  공통 여섯 시험은 OS cfg로 빼지 않는다. Unix 실행 권한/harness 무실행 하나는 chmod·shell 의미를 검사하므로 Windows에 없다.
  여기에 OS별 native owner 하나를 더해 Mac/Linux 8개, Windows 7개다. Mac은 기존 LaunchAgent 파일,
  Linux는 stat mount 관찰·systemd system 조회/booted marker·XDG user unit 충돌·상대 경로,
  Windows는 실제 drive mount·SCM 읽기 조회·drive 상대/불완전 UNC 거부·ACL unknown을 검사한다. 등록/daemon-reload는 하지 않는다.
  snapshot은 내용·목록뿐 아니라 파일 종류·권한·수정 시각을 포함해 같은 내용 재쓰기/권한 변경도 잡는다. I0 순수 입력 표를 반복하지 않는다.
  install 앞의 malformed --json=value/모르는 전역 옵션과 malformed stream flag도 정제된 envelope여야 한다. 실제 fake secret stderr 유출 RED 뒤 명령 트리 metadata 기반 판별로 수리했다. 다른 명령의 값 install은 기존 오류 계약을 유지한다.
- 기존 base/local 설정·CLI에 없는 reconnect 변경의 계획 무효화, fresh 충돌, 없는 mount/남은 일반 폴더,
  binary SHA/형식·선택 파일 무실행, 실제 Git rev-parse 전후 무변경, profile import의 secret env/자유 실행 거부를 확인한다.
  모든 child 출력에서 fake secret canary가 없어야 한다. snapshot에는 선택한 events/lock/probe 경로도 들어간다.
  profile owner는 실제 빌드 볼륨에 정상 파일을 두어 macOS 외장 볼륨에서도 읽기 계획이 성공하는지 본다.
  storage owner는 기존 일반 파일 work-root를 exit 2로 거부한다. 두 실제 회귀는 수정 전 RED·수정 후 GREEN으로 남긴다.
- 무변경 owner에 mkdir, 문법 오류 owner에 argv 출력, hash owner에 실효 context 무시,
  profile owner에 secret/shell guard 제거, harness owner에 --version 실행을 각각 심으면 의도한 단언에서 실패해야 한다.
  원본 source bytes를 복구하고 같은 owner GREEN 뒤 전수 gate를 실행한다. 의도적인 leak 양성 대조 로그와 정상 출력 검사를 구분한다.
- 공식 gate는 전체 ci-local·deps·check-public이다. Windows cross-check는 native SID/volume API의 컴파일 증거다.
  Windows·Linux 실제 실행: main push 뒤 CI(windows-2025·ubuntu-24.04)의 cargo test가 판정한다.
  사전 branch/PR push·별도 runner/VM은 쓰지 않는다. 착지 전은 macOS 전체 gate와 Windows cross clippy/build이며,
  I1을 완료로 부르지 않는다. 서비스 등록·boot/login/PTY는 후속 단계의 별도 증거다.

## 자동화 A2: 이어 보내기·기억·짧은 끝내기(t-0504)

- host의 `automation::start_tests` fixture로 최근 열린 thread, propose의 이어 보내기, 33건의 32+1 전달, receipt, Codex 거부와 보고 첨부의 64KiB와 단일 입력의 512KiB 초과 거부를 검사한다. 종료 owner 판정표는 표지 6종·보고/입력 시각·활동·내구/파일 입력·검토/쉼을 교차한다. 기존 report boundary keeper는 자동화 완료의 앱 안 알림 없음과 확인 필요 알림 유지를 함께 본다.
- m13 `ci::automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory`: 서로 다른 fake CI 두 건을 batch에 보존 → 한 thread → 셋째 Queue 전달 → 옛 완료로 끝내지 않음 → 새 완료 뒤 resolve/기억 사본 → 다음 thread 기억 → 전달 모름/무재전송. FIFO로 화면 진행을 허용하며 host가 Working/Idle를 관측한 뒤 다음 단계로 간다.
- 자동 완료의 첫 확인은 보고 hash·Idle와 함께 `acked_hash == report_hash` 기록을 기다린다.
  보고 hash 저장과 자동 ack 저장은 별도 DB 갱신이므로 앞 둘만으로 확인 완료를 추정하지 않는다.
  t-0679는 자동 ack 직전에 임시 FIFO를 삽입하고 hash·Idle·ack 미기록을 실제 조회한 뒤 해제해,
  기존 단언의 `Null != 보고 hash` 실패와 수정한 시험 전체의 통과를 확인했다.
  주입 코드는 제품에 남기지 않으며 기존 30초 한도·hash 일치 단언을 유지한다.
  같은 시험의 입력 파일/묶음 수는 기동·전달 기록, 옛 완료의 미종료는 ticker 완료,
  기억·충돌 사본은 마지막 회수 뒤 resolved, 다음 기억은 brief 전달,
  무재전송은 막힘 기록과 후속 ticker 완료를 각각 관측한 뒤 검사한다.
- `AAA_TEST_AUTOMATION_CLOCK_FILE`에 Unix ms를 쓰면 자동화 batch·close_idle의 판정 시계만 움직인다. 실제 보고/입력 시각과 agent 활동 시각은 바꾸지 않는다. m13 Lab의 설정 폴더 안 파일만 사용한다. 고정 sleep·timeout 연장은 없다.
- E2E49 `49-automation-memory`: 실제 report/Idle 사건으로 기억/크기/마지막 갱신 thread와 끝남 예정 표시를 본다. 데스크톱·390px에서 상세를 열고 캡처하며 가로 넘침과 자동화 완료 droplet 없음도 검사한다.
- 결함 되심기: close_at의 마지막 입력보다 뒤인 보고 조건(시각과 입력 전 hash)을 제거하면 m13의 옛 완료 보호 단계에서 open 대신 resolved로 실패한다. 원본을 바이트 단위로 복구한다.

### 자동화 기억의 동시 편집(t-0548, plan §8.350)

- `cargo test -p aaa-host --lib memory`는 실제 host·DB와 새 work_files/N-1 Mission frame fixture로 전달 뒤 host 덧붙이기와 thread 요약 변경을 겹친다. host 줄·양쪽 보존 사본·history 두 쪽·충돌 inbox 한 건을 검사하며, 저장한 공통 판을 새 host owner가 읽고 오래된 ThreadRec으로 반복 회수해도 원문을 덮지 않아야 한다. 각각 상한 아래인 양쪽 history를 합쳤을 때 1000개를 넘으면 회수를 거부하고 양쪽 원문을 유지해야 한다. 원래 제품은 「열린 동안 host에서 쓴 줄이 회수로 사라졌다」에서 실패했다.
- 순수 판정은 독립 추가·삭제, 같은 변경, 수정/삭제 충돌, 기준 없는 옛 실행, 파일/폴더 경로 충돌, 되풀이한 압축, host만 고친 뒤 나중에 thread도 고치는 순서와 수동 해소를 검사한다.
- 기존 m13 `ci::automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory`가 실제 host→agent→PTY의 FIFO gate를 잡고 마지막 보고 전에 host 줄을 더한다. thread는 기존 기억을 history로 옮긴 뒤 요약한다. resolved 사건 뒤 host 원문·4KiB 이하 두 요약·history·양쪽 사본·중복 보존 없음·inbox를 확인하고, 다음 실행이 host의 보존된 기억을 받는지 검사한다. 고정 sleep과 시한 변경은 없다. 이 실험실은 운영 자동화 파일을 읽거나 고치지 않는다.

## host 계정 전달과 동시 갱신(t-0500, plan §8.320)

`cargo test -p aaa --test m13 accounts:: --locked`는 빈 포트·임시 폴더·가짜 인증만 사용한다.
CLI→host→UDS/TLS agent→session pipe→PTY의 실제 경계에서 같은 Codex 계정 네 작업(머신당 둘)을
동시에 띄운다. 이전 refresh token을 재사용하면 거부하는 가짜 제공자가 두 회전을 처리하며,
각 회전의 제공자 호출은 한 번이다. 부분 응답의 이전 토큰 보존, 갱신 거부 시 우회 없음, 대화 조회·
같은 기록으로 재개, auth 파일 권한·정상 종료 청소를 검사한다. Claude 동시 작업·기존 profile 인증·
계정 미지정 환경도 함께 확인한다. host 단절 중 outbox를 직접 검사하고 DB/WAL·post·spec·준비
journal·로그·화면에 원문·8바이트 조각·base64/URL/JSON 변형이 없는지 찾는다.

금고의 암호문/AAD 무결성·키 잠김은 `cargo test -p aaa-host accounts:: --locked`가 소유한다.
이 시험은 실제 로그인·실제 인증 파일·운영 host/agent·기본 설정에 접근하지 않는다.

## 대화창 컨텍스트 게이지(t-0502, plan §8.321)

- `cargo test -p aaa-agent --lib plan::tests::`: 대화 기록 꼴(Claude assistant `usage`·`requestedModel`·`compact_boundary`, Codex `token_count`·`compacted`)을 임시 파일에 이어 적고 Reader를 직접 돌린다. 값은 실제 harness 실측(Claude 43,922 → 7,910, Codex 18,861 → 12,274)이다.
- `node --test web/test/contextUsage.test.mjs web/test/inputReceipt.test.mjs`: 표기(%·k·1050k), 치는 동안 숨김 규칙(80% 미만·창 모름만), harness 명령 한 줄 판정.
- `node web/e2e/web.e2e.mjs --spec 50-context-gauge`:
  - 가짜 Claude가 SessionStart hook으로 기록 자리를 알린다. F7(작업 중)·F5(쉼)·F6(정리 끝: `compact_boundary` pre 163,600/post 28,400과 요약)과 「/compact」 Enter(정리 화면)를 받는다.
  - spec이 기록에 assistant usage 줄을 더하면 게이지 글이 바뀔 때까지 `domIs`로 기다린다(agent는 2초마다 읽는다).
  - 패드·데스크톱의 치는 중 숨김은 `data-hidden`과 게이지·글 칸 폭이 같은지로 본다. 정리 단추는 pane이 받은 bytes와 입력칸 잠금 풀림으로 본다. 고정 sleep은 없다.
  - t-0570(사용자 10-21): WebKit iPhone에서 빈 입력칸·focus만 있음·치는 중·80% 이상·창 넘음·창 모름 모두 게이지가 보이지 않고 폭도 0이며 접근성 트리에서도 빠지는지 본다. 320·390·700px는 숨고 701·768px는 보인다. 실제 iPad 꼴(820px)에서는 기존 표시·치는 중 숨김·펼침을 유지한다.
  - 첫 인자로 캡처 폴더를 주면 데스크톱·iPhone 밝음/어두움과 기존 데스크톱 펼침·정리 중·정리 뒤, iPad 펼침 그림을 남긴다. 테마는 실험실 명령 팔레트로 바꾸고 DOM 반영을 기다린다.

## aaa install I2 파일·상태 owner(t-0485, plan §8.322)

- `cargo test -p aaa --bin aaa install::engine::transaction --locked`: 실험실 실제 파일을 stage/commit하고
  rename 뒤 journal 전 중단, 같은 ID 재개, 외부 편집/같은 내용의 외부 inode 교체, 두 프로세스 OS 잠금,
  다른 instance, 반복 unchanged의 bytes/권한/mtime 불변, 공유 마지막 참조 제거/원본 hook 복원,
  제거·rollback 중단, missing repair·reinstall 데이터 보존, 과거 ID 재개 거부, 같은 config의 별도 파일/job 잠금,
  root 보존 partial/remaining, 소유 stage 정리·외부 stage 보존, 프로젝트 뼈대/기존 repo를 검사한다.
  실패 주입 flag/export 없이 실제 파일 작업과 production 단계 경계를 사용한다.
- 일반 journal/공개 결과에는 fake secret이 없어야 하고 원본 backup은 별도 private 파일이어야 한다.
  status는 실제 파일·root 소유 기록을 읽으며 lock/probe/기록 보정 없이 전후 snapshot이 같아야 한다.
  Unix owner는 mode/소유권/nlink/symlink를, Windows 전용 owner는 실제 NTFS hardlink와 열린 DACL을
  사용한다. 공통 파일 owner는 OS cfg로 빼지 않는다. Unix 전용 1개는 Unix mode/symlink를 검사하며
  Windows는 그 대신 handle link 수와 현재 SID+SYSTEM DACL을 실제 CI에서 검사한다.
- `cargo test -p aaa --test install --locked`: I1 owner에 status routing과 `--yes`/임의 승인 JSON의
  쓰기 차단을 더한다. 자동·SSH·외부 TTY apply/resume은 검증된 I6 승인 경로 전까지 needs_user/3이다.
- baseline/rename 소유 증명·잠금·private 읽기·마지막 참조·uninstall 편집 대조·reinstall TOML 보존·active job·
  repo 보존·root 읽기 대조·일반 journal 비밀 비노출의 결함 주입이 해당 owner에서 실패해야 한다.
  변형은 bytes로 복원하고 원본 owner 통과 뒤 전체 gate를 돌린다. Windows/Linux 실제 실행은
  main push 뒤 GitHub CI이며 cross clippy/build를 실제 실행 통과로 쓰지 않는다.

## 입력칸 「/」 목록과 명령 줄(t-0502, plan §8.323)

- `cargo test -p aaa-agent --lib -- slash:: chat::tests::claude_commands plan::`: 임시 폴더에 명령·skill 파일과 실측 기록 줄을 두고, 대화 줄은 실제 쪽 읽기(`read_path`)로 읽는다. 창 크기 시험은 사용자 설정 파일 자리를 임시 경로로 바꿔 실제 `~/.claude`를 읽지 않는다.
- `node --test web/test/slashCommands.test.mjs`: 여는 자리, 순위, 숨김, 확인창·알림 판정.
- `node web/e2e/web.e2e.mjs --spec 51-slash-commands`:
  - 실험실 `CLAUDE_CONFIG_DIR`의 `commands/`·`skills/`와 pane 폴더(`.git`이 있는 git root)의 `.claude/commands/`를 agent가 읽는다.
  - 가짜 Claude는 F7(작업 중)·F5(쉼)과 Enter 글을 받는다. `/context`는 실측 꼴의 명령·출력 줄을 적고, `/model`은 선택창을 그렸다가 Esc에 결과를 적고, `/clear`는 새 기록으로 SessionStart(clear) hook을 보낸다.
  - 「보내지 않았다」는 pane이 받은 bytes로, 상태 전이는 가짜가 그린 「mode …」를 `waitOutput`으로, 화면은 `domIs`로 기다린다. 휴대폰은 WebKit `pairDevice`. 고정 sleep은 없다.
  - 지연 커서 회귀(t-0536, plan §8.338)는 다음 RAF를 보류한 채 Tab 명령 넣기·↑ 기록 부르기 뒤 실제 키로 전체 선택하거나 중간에 글을 치거나 focus를 옮긴다.
    보류한 콜백이 실제 frame에서 모두 끝난 뒤 글·선택·focus가 그대로인지, 전체 선택 뒤 Delete가 비우는지 검사한다.
    원래 RAF 커서 복원을 되살리면 새 전체 선택이 끝 커서로 바뀌어 실패해야 한다. 글과 커서는 같은 React commit에서 반영한다.
    이 scheduler keeper와 별도 transport 지연 진단은 따로 실행한다(transport의 Tab frame 대기를 keeper가 보류하면 시험 도구끼리 막힌다).

## 자동화 A4: 특수 실행 제거(t-0507)

- A4 뒤 남은 종료 pane(t-0545, plan §8.346): `web/test/roster.test.mjs`는 옛 평가자·임무의 `evaluator` pane 18개가 「기타」·단축키에서 빠지고, 실행 중·중단·유실·사용자 종료 기록과 연결된 thread 줄은 남는지 본다. `web/e2e/specs/57-legacy-special-panes.mjs`는 격리된 실제 agent의 옛 세션 파일을 복원해 화면에서 18개가 빠지는지, 실제 실행 중인 사용자·평가자 pane과 사용자 종료 기록이 남는지 검사한다. t-0540·W4의 막대 단추 다섯과 「답할 것」→「알림함」→「작업」 전환 뒤에도 같은 숨김을 확인한다. 숨긴 기록의 파일 바이트·CLI 읽기·주소로 직접 보기를 함께 확인한다. 종료 기록은 지우지 않으며 새 변환 명령 없이 목록에서만 숨긴다. 준비 완료·종료·출력·DOM 사건으로 끝낸다.
- `cargo test -p aaa --test m13 automation_start::legacy_mission_files_are_inert_and_convert_with_memory_and_pending_input -- --exact`: 실제 host·agent·CLI·Front API에서 옛 파일 비실행, 완료 처리 뒤에도 한 번인 제거 안내, doctor 진단, mission_list 빈 목록/mission_start 제거 오류, 열린 옛 evaluator 표시, 미리보기 무쓰기, 기억·이력 hash 회수, 원본 보존·대기 입력 이전·tick 뒤 무복제를 확인한다.
- `cargo test -p aaa-host previous_agent_keeps_input_and_memory_transfer_with_legacy_file_frames --lib`: AGENT_LINK 1.9/mission.v1 fake가 입력 쓰기와 기억 주입·회수·처리 여부 조회를 받는다. 새 WorkFile 태그가 옛 상대에 닿으면 실패한다. 실제 agent의 파일·hash·symlink keeper는 `threads::work_file_tests`가 계속 맡는다.
- E2E26은 패널 셋과 저장된 special→work를, E2E40은 승인 규칙+막힌 규칙 배지와 기존 변환/Reader/큐 풀기를 검사한다. E2E48·49는 기존 자동화 줄과 기억을 유지한다. 캡처는 데스크톱·390px, 실제 host·agent를 사용한다.
- `Role::Evaluator`와 구 Front/Agent DTO는 한 판 호환 경계에만 남는다. 옛 outbox의 모르는 칸과 ThreadRec의 원본 데이터는 변환 전까지 보존하며, L0 snapshot keeper의 기대 문서는 바꾸지 않는다.

I2 Windows 수리(plan §8.325): 같은 파일 owner를 세 OS에서 유지한다. `resource_locks_serialize_processes`는
실제 자식 process 둘(같은 작업/같은 config의 다른 파일)을 거부하면서, 잠금 중 유효 status와 파일 snapshot이
읽히고 바뀌지 않으며 해제 뒤 같은 ID 재개가 되는지 함께 본다. Windows는 읽기 공유만 허용하는 handle
예약을 사용한다. stage/reverse flush는 Windows 쓰기 handle로 수행하고 원본 ACL/hardlink/복구 owner를 뺄 수 없다.
Windows 실행이 없는 현지 cross 결과는 native GREEN이 아니며 다음 main CI 결과와 나눠 보고한다.


## 웹 할 일 첨부(t-0506, plan §8.327)

- 웹 E2E 52 `task-attach`가 실제 host·agent에서 Chromium 데스크톱·iPhone·iPad(밝음·어두움)로 붙여 넣기(⌘V·붙여넣기 칸), 고치는 중 초안,
  떼기, md 올리기·보고서 문서 고르기·주소, 파일 보기 열기(md는 그린 Markdown, html은 sandbox 페이지), 「바로 시작」 복사, 「할 일에 추가」
  (보고서 머리·thread 결과물 줄 → 새 task·있는 task에 링크), 보고서 주소 알아보기(공유 링크·앱 보고서 주소·풀 수 없는 주소), 보기 권한을
  본다. 올린 html은 GET 응답의 sandbox CSP·`nosniff`·글 종류를 본다. 끝은 화면 표지(`data-tasks-sha`·`data-count`)와 TASKS.md·작업
  폴더 파일로 정한다. 시간은 기다리지 않는다.
- iOS 길게 누름 메뉴와 클립보드 허용 말풍선은 headless가 그리지 못한다. 그 메뉴 뒤 브라우저가 내는 `ClipboardEvent`를 붙여넣기 칸에
  재생한다. iPhone 실기기 확인은 배포 뒤 사용자가 한다(보고의 절차).
- 휴대폰 단계는 상세 시트 위 파일 보기를 닫기 단추로 닫은 뒤 상세가 남고, 다른 화면으로 가도 파일 보기가 되살아나지 않는지 본다.
  상세 시트가 파일 보기 동안 기록을 내주지 않으면(`usePhoneSheetBack(…, !fileOpen)`을 빼면) 이 단계가 실패한다.
- 웹 순수 시험 `web/test/taskAttach.test.mjs`는 첨부 줄 규칙·입력칸 글·붙인 글과 파일 판정(주소의 괄호는 풀어 쓰고 괄호 든 보고서
  경로는 받지 않는다)·「할 일에 추가」 단추를 둘 문서·보고서 주소 꼴을 본다.
- host `task_attach::tests::upload_rules`는 보기 권한·폐기한 기기를 파일을 쓰기 전에 거부하고(옛 순서는 폐기 기기의 파일을 남겼다),
  signature 없는 그림·NUL·UTF-8 아닌 문서·모르는 문서 종류를 거부하며, 같은 내용은 파일 하나, 읽기는 이름 꼴만·symlink 거부인지 본다.
- host `library_links::tests::task_link_resolves_report_urls_and_reads_live_docs`는 공유 링크·앱 보기 경로의 cap 풀기, 보기 권한·
  다른 프로젝트·없는 cap·폐기 거부, live 공개 문서 사본 읽기를 본다. `task_attach::tests::old_agent_is_refused_before_a_record`는
  `work_attach.v1`이 없는 agent에 thread 기록을 만들기 전에 거부하는지 본다(앞 확인을 빼면 실패한다).
- m13 `task_attachments_reach_a_remote_agent`는 TLS 원격 agent(다른 상태 폴더 labB)에 `--from-task`로 띄워 그림·보고서 문서를
  그 작업 폴더 `attachments/`에 같은 바이트로 쓰고, 할 일 글이 그 경로만 가리키며 host에 없는 첨부는 경고로 알리는지 본다. 이어서
  host 원본(올린 그림·보고서 문서)을 지운 뒤에도 `thread restart`가 되고 첫 사본이 남는지 본다. 첫 전송의 `sent` 표지를 저장하지
  않으면 이 다시 시작은 「아직 쓴 적 없는 첨부」로 보고 멈추므로 실패한다(표지 저장을 빼고 확인했다).

## task 저장소 칸(t-0506, plan §8.330)

- m13 `task_repo_field_picks_machine_and_path`는 labA(host 로컬)·labB(TLS 원격)에 이름이 같은 저장소(경로는 다름)를 project.toml repos로
  두고, owner 머신이 정한 task는 그 머신 경로의 worktree로, 칸만 있는 task는 열린 thread가 적은 머신(labA)으로 띄우는지 본다.
  덜 붐빔 차례를 뒤집으면 labB를 골라 실패한다(결함을 넣어 확인했다). 어긋남(`--repo`·`--kind`·없는 머신·repos에 없는 이름) 거부,
  모르는 칸의 형식 어긋남, `task add|edit --repo`(경로 → 이름, 빈 글 비움, 모르는 이름 거부), 같은 머신에 같은 이름 저장소가 둘일 때의
  거부와 끝 `/`를 붙인 `--repo` 경로로 띄우기도 본다.
- 웹 E2E 54 `task-repo`는 labA 하나로 폼 「저장소」 고르기와 칩, 칸이 있는 task의 고정된 저장소·머신과 worktree 시작, 칸이 빈 task의
  「고르기…」(시작 막힘) → 작업 폴더 시작과 줄에 적지 않음(iPhone)을 본다. 두 머신 사이 고르기는 m13과 웹 순수 시험이 맡는다.
- 칸이 빈 task를 웹에서 시작하는 다른 spec(47·52)은 「작업 폴더」를 고른 뒤 시작한다(결정 4A).

I2 Windows 잔여 수리(plan §8.331): `unsafe_targets_readonly_and_untrusted_journals_preserve_originals`의
ParentDir 입력은 canonical Windows prefix에서도 `..`가 남도록 native separator를 OsString에 raw로 붙인다.
fixture의 ParentDir 전제조건을 선행 단언한 뒤 target의 conflict 거부·escape 미생성·전체 tree 불변을 확인한다.
`Path::join`은 Windows verbatim prefix에서 `..`를 먼저 정규화하므로 이 부정 입력을 만드는 데 쓰지 않는다.
기존 invalid journal status의 bytes/권한/mtime·비밀 비노출 검사를 유지하며, 거부 분기를 허용으로 바꾼
결함 변형은 conflict 단언에서 실패해야 한다. Windows 실제 통과 판정은 main CI가 맡는다.

## 비밀 없는 panic 위치(t-0533)

- `cargo test -p aaa-common --lib panic_log::`는 실제 hook을 별도 시험 프로세스에서 설치한다.
  CLI·host·agent·session host 네 종류마다 비밀 String·비문자열 payload·동적 thread 이름·환경변수를 두고,
  `RUST_BACKTRACE=full`이어도 stdout/stderr와 알림용 snapshot에 비밀이 없는지 검사한다.
  같은 소스 위치를 서로 다른 thread에서 세 번 실행해 횟수 1·2·3, 다른 위치는 1,
  실제 Tokio task의 숫자 ID와 파일·줄·열·빌드 SHA를 확인한다. stderr를 실제로 잠근 동안에도
  감시자가 `latest()`를 읽는 사건을 받아야 한다. 종료·join·관측 사건으로 끝내며 sleep은 없다.
- `panic_log::latest()`는 마지막 안전한 `Snapshot`을 반환한다. hook은 알림을 전송하지 않는다.
  단일 실행 파일 진입점에서 hook을 설치하고, 파싱한 명령으로 프로세스 종류를 정한다.
  등록용 `agent enroll` 등 일반 명령은 CLI, 복구 감시는 agent로 분류한다.
- 임의 thread 이름은 `other`로 숨기고 런타임 숫자 ID로 구분한다. 고정 이름(main·aaa-cli·Tokio worker)만 허용한다.
  Tokio의 안정 API에는 task 이름이 없어 숫자 task ID를 쓴다. payload·환경·backtrace는 읽거나 포맷하지 않는다.
- build script는 Git HEAD SHA와 추적된 Rust 파일 목록만 생성물에 넣는다. HEAD·현재 ref·index·packed-refs를 감시한다.
  절대 경로는 허용된 저장소 상대 경로로 줄이고, 모르는 의존성·생성 파일 위치는 `external`로 숨긴다.
  Git 없는 소스 배포는 SHA `unknown`, 경로 `external`로 숨긴다. 줄·열과 원본 위치별 횟수는 계속 남긴다.
- 본문 출력 복원·원본 경로 출력·반복 횟수 초기화 결함은 각각 keeper의 비밀 비노출·경로·횟수 단언에서 RED여야 한다.
  stderr 쓰기 중 상태 잠금을 잡는 결함은 감시자가 출력 전에 위치를 읽는 계약에서 RED여야 한다.
  각 변형의 원본 bytes를 복구한 뒤 GREEN과 전체 ci-local·deps·TS 최신·check-public을 확인한다.

## aaa install I5 Windows 감독자(t-0485, plan §8.333)

- `cargo test -p aaa --bin aaa install::engine::supervisor --locked`: 공통 owner 7개는 세 OS에서 실행한다.
  실제 I2 파일/receipt/잠금·감독자 journal로 등록 직후 중단/실제 manager 대조·같은 ID 재개·기존 암호 보존·
  외부 서비스 보존·credential 중단/이전 process 미채택·보존 제거·읽기 status를 확인한다.
- lifecycle owner는 별도 process의 실제 고정 host/agent backend와 실제 ready producer를 쓴다.
  host가 없는 agent 초기화 → 실제 host 연결/Welcome → 단절/재연결 → 각각 stop을 사건으로 기다린다.
  유효 proof를 받아 계정/context/SHA/PID/생성 시각/boot/session/연결/elevation을 변조해 거부를 본다.
  실제 TCP listener 충돌은 RUNNING/ready를 낼 수 없다. 외부 SCM/token 관찰만 모형이고 준비 영수증을 fixture가 만들지 않는다.
- Windows 추가 owner 4개는 실제 현재 token/profile/image/SCM query·EventLog 읽기 구독/해제 ABI·CommandLineToArgvW,
  종료 process handle 거부·실제 configured I2 receipt와 context/active ID 대조다. Win32 API가 없는 Mac/Linux에서 이 ABI owner만 제외한다.
  lifecycle Windows 자식은 CREATE_NO_WINDOW지만 실제 SCM Session0 logon 증거는 아니다.
- `cargo test -p aaa --test install --locked`는 숨은 서비스 진입의 console fallback 거부와 무변경을 포함한다.
  apply/resume 승인 차단·I1 읽기 keeper를 유지한다. agent link owner는 Welcome/replay 완료 전 연결을 공개하지 않는지 본다.
- 실제 process 대조 제거·proof context 무시·credential 이전 process 허용·manifest 결속 제거·LSA Deny 무시·
  Removed에서 실행 파일 강제 요구·restart stop 생략·listener 전 host ready 변형은 각각 기존 owner에서 실패해야 한다.
  원본 bytes 복구 뒤 공통 owner GREEN을 확인하고 전수 gate를 실행한다. 비밀 canary는 private tree UTF-8/UTF-16와
  정상 owner 출력에서 검사한다. 시험 허용 production flag/export·실제 SCM/LSA 쓰기·고정 sleep·재시도는 없다.
- 전체 macOS ci-local·deps·check-public과 Windows workspace/all-targets cross Clippy/build를 구분해 기록한다.
  Windows·Linux 실제 실행은 main push 뒤 CI(windows-2025·ubuntu-24.04)다. 실제 서비스 logon·부팅·권한·암호·
  ConPTY/공식 인증은 Windows bootstrap 뒤 사용자 실기 인수의 준비물/명령/되돌리기/로그 항목으로 남긴다.

## host 핵심 작업 감시(t-0532, plan §8.334)

- `cargo test -p aaa-host --lib supervisor::tests --locked`: 실제 task의 panic/조기 종료 사건과 가상 backoff 시각으로 1·2·4초 뒤 세 번 재실행, 상한 중단, 한 번 알림을 검사한다. loopback HTTP 수신은 앱을 닫았을 때 이상·회복 각각 한 번, 앱을 쓰는 동안은 notice만 확인한다. 이상/회복 판정 저장 뒤 inbox commit 전 재시작, SQLite INSERT 실패 뒤 재개방, failed INSERT만 거부해 뒤 recovered가 기다리는 순서, 이미 발행한 이상의 재시작 무중복·회복 이어 알림도 같은 owner가 검사한다. 판정의 `now`로 30초 안정 경계·희소 시간 초과 만료·회복 중 새 시간 초과를 확인한다.
- `cargo test -p aaa-host --lib link::supervision_tests --locked`: Hello/Welcome/Ping을 마친 실험실 link의 전송체가 읽기 중 panic한다. pending의 즉시 AgentDisconnected·세대별 연결 정리·재연결 처리·네 번째 종료 뒤 거부와 독립 감시기의 inbox/notice 한 번을 확인한다. 다른 keeper는 실제 AgentConn 요청을 받되 응답을 보류하여 다섯 deadline 사건을 만들고, 이후 같은 reader의 실제 응답을 확인한다. ticker는 띄우지 않으므로 전달이 ticker에 기대면 실패한다. panic 감지 경계는 공통 `panic_log::latest()` API를 호출하고, 안전한 위치·payload 비노출의 정본 keeper는 t-0533의 `panic_log::` 시험이다.
- 감시 제거·pending 정리 제거·재실행 상한 변경·시간 초과 관측 제거·회복 판정 제거·중복 억제 제거를 각각 되심으면 owner 단언이 실패해야 한다. 변형마다 원본 바이트를 복원하고 원본 keeper를 통과시킨 뒤 전체 ci-local/deps/TS/public gate를 수행한다. 실제 OS/Web Push 제공자의 휴대폰 전달·host 프로세스 전체 정지는 이 실험실 keeper의 보증 범위 밖이다.

## 계정 thread 폴더 신뢰(t-0543)

`cargo test -p aaa --test m13 accounts::account_threads_trust_only_their_cwd_when_coordinator_answers -- --exact`
는 실제 host·로컬/원격 agent·session·PTY와 임시 홈의 가짜 Claude/Codex를 사용한다.
두 harness × 두 머신 × worktree/일반 작업 폴더에서 `trust_screens=coordinator`일 때 신뢰 기록은
정규화한 자기 cwd 한 곳뿐이고, `user`일 때 기록이 비고 실제 신뢰 화면이 남는지 확인한다.
Claude SessionStart가 알린 격리 홈의 대화는 pane chat으로, TaskCreate 계획은 thread 상태로 확인한다. 완료는 socket gate와 종료 사건으로 판정한다.
실제 인증 파일·키체인·제공자 네트워크는 사용하지 않는다. host의 정책 분기나 session의 기록 생성을
빼면 시작 증명의 trusted 단언에서 실패해야 한다. 정책을 무시하고 늘 기록하면 user 단언에서 실패해야 한다.
`cargo test -p aaa-session --lib account::tests::`는 격리 Claude의 환경설정 보존,
재개 시 자동 신뢰 제거, 도입 전 Claude 대화의 원래 홈 유지도 확인한다.

## aaa install I2 파일 조상 경로(t-0485, plan §8.341)

- `unsafe_targets_readonly_and_untrusted_journals_preserve_originals`는 세 OS 공통 owner다.
  기존 readonly stage/원본 bytes, 불신 journal 읽기의 exit·비밀 비노출·tree 불변, raw ParentDir 거부를 유지한다.
  파일 아래 직접 자식/여러 단계 proposal은 거부하고 정상 미생성 디렉터리 아래 proposal은 tree 변경 없이 허용한다.
  유효 proposal 뒤 부모를 일반 파일로 교체한 실제 `Transaction::create`는 journal과 다른 자원 잠금 생성 전에 거부한다.
- 기존 Windows CI 실패는 native RED다. Unix의 NotADirectory를 Windows식 NotFound로 임시 분류한 진단은
  native Windows 실행으로 부르지 않는다. 시험용 production flag/export를 만들지 않고 원본 bytes를 복구한다.
- snapshot 부모 검사·가장 가까운 조상 검사 생략은 오류 분류 진단에서 proposal 거부 단언을 실패시켜야 한다.
  실제 create의 쓰기 전 확인 생략은 native owner의 전체 tree 보존 단언을 실패시켜야 한다.
  각 변형 복구 뒤 엔진 owner·전체 ci-local·Windows cross와 비밀 canary 비노출을 확인한다.
- 동반 Windows native owner `process_observation_rejects_completed_process_with_its_handle_still_open`은
  기존 cmd/인수/환경/stdio/creation flags와 success·signaled process·유효한 열린 handle의 거부 단언을 유지한다.
  실패 메시지에 종료 코드(Debug/십진수/32-bit hex)만 더해 main CI가 원인 진단 값을 남기게 한다.
  native는 진단만, 수리는 다음 착지다. macOS와 cross 통과를 이 Windows owner의 실제 GREEN으로 부르지 않는다.

## project 명령의 프로젝트 이름 인자 정리(S5, §8.344)

- CLI 유닛 `project_lifecycle_help_omits_legacy_project_arguments`: 여섯 명령의 도움말에서 프로젝트·호환 인자를 숨기며, 새 꼴과 옛 꼴만 받고 넘치는 인자는 거부한다. rename의 새 이름은 여전히 필수다.
- m13 `project_lifecycle`: 이름 없는 pause·resume·archive·rename·delete·sweep으로 실제 상태 전이·거부·이름 변경·기록 이동·휴지통·worktree와 병합된 branch 청소를 본다. 보관한 프로젝트를 직접 지정하는 호환 경로도 유지한다.
- m13 `project_cli_keeps_legacy_names_with_a_stderr_warning`: 옛 꼴 여섯 명령은 경고 한 줄을 표준 오류에만 쓰고 JSON은 그대로 파싱된다. 다른 이름은 모두 거부하며 상태를 바꾸지 않는다. 새 꼴 sweep은 경고 없이 같은 결과, 옛 꼴 rename은 실제 이름을 바꾸고 새 꼴로 되돌린다. 보관 뒤 이름 생략은 기존 오류이며 폴더를 보존한다.
- pause에서 주어진 이름을 무시하고 현재 프로젝트를 쓰는 결함을 심으면 다른 이름 거부 단언에서 실패한다. 원본 바이트를 복원한 뒤 같은 keeper를 다시 확인한다.
- m13 `coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`·`project_lifecycle`: 새로 만들거나 doctor로 갱신하거나 이름을 바꾼 AGENTS.md에 이름 없는 pause·resume·sweep 안내가 들어간다.
- 실험실 host·가짜 agent만 쓰며 운영 프로젝트·설정·host는 건드리지 않는다. 고정 sleep이나 새 시험 전용 제품 경로는 더하지 않는다.


## 계정 Claude 첫 실행 우회 경고(t-0551, plan §8.347)

- `cargo test -p aaa --test m13 accounts::fresh_account_claude_initializes_only_authorized_bypass_consent --locked -- --exact`는 실제 host·로컬/원격 agent·session·PTY와 가짜 Claude를 사용한다. yolo 켜기/끄기, profile의 두 permission-mode 표기와 일반 모드로 새 홈 10개를 시작한다. 안내 완료와 우회 수락 키, 첫 경고 부재, 가짜 원본 홈의 인증·다른 설정 비복사와 원본 불변을 확인한다. 폴더 신뢰는 별도 정책이므로 실험실에 `trust_screens=coordinator`를 명시한다. 기본값을 coordinator로 가정했던 첫 시험 실패는 제품 결함으로 분류하지 않는다.
- 가짜 시작 화면은 설치된 공식 Claude 2.1.304의 시작 조건과 저장 키를 따른다. 실제 제공자 CLI·키체인·인증 파일·외부 로그인은 실행하지 않는다. 완료는 socket gate·harness 종료 사건으로 판정하며 고정 sleep·재시도는 없다. 이 시험이 실제 제공자 인증이나 이후 새 버전의 추가 안내를 보증하지는 않는다.
- `cargo test -p aaa-session --lib account::tests:: --locked`는 명시적 우회/허용 옵션, 마지막 permission-mode, `--` 뒤 문자열 제외, 일반 실행의 파일 미생성, 기존 환경설정 보존, 비우회 재개 시 자동 수락 제거, 인증 비기록, Unix 0600·symlink 대상 보존·깨진 JSON 거부를 확인한다.
- 수락 준비 제거와 비우회에도 무조건 수락하는 변형은 각각 위 m13의 settings 단언에서 RED여야 한다. 매번 원본 bytes를 복구한 뒤 정상 owner와 전체 ci-local·check-public을 실행한다.


## install I5 Windows 종료 process fixture(t-0485)

`install::engine::supervisor::native::tests::process_observation_rejects_completed_process_with_its_handle_still_open`은
현재 시험 실행 파일을 `--list`로 직접 실행해 시험 본체의 실행·재귀 없이 성공 종료시킨다(§8.356).
cmd 인수 분리(§8.352)는 실제 Windows CI에서도 실패했으므로 셸 해석에 기대지 않는다.
stdout은 null로 두고 stderr는 reader thread가 읽어 자식 종료를 막지 않으며, 실패하면 status·code·32-bit hex·stderr 첫 줄을
panic 앞 `eprintln!` 한 줄로 남긴다. `wait_with_output`으로 Child를 소비하지 않아 같은 process handle이 남는다.
자식의 성공 종료·아직 열린 유효 handle의 `WAIT_OBJECT_0`·제품 `service_process_unverified`를 모두 검사하며
고정 sleep·재시도·timeout·production 변경은 없다. 같은 native/상위 supervisor의 현재 image·SID·profile·SCM 읽기와
Unicode argv 왕복 owner도 Windows cargo test에 남긴다. 이들은 Win32 전용이며 실제 실행은 main push 뒤 windows-2025,
macOS gate·Windows cross compile 결과와 별도로 보고한다. SCM 실제 logon·UAC/LSA는 실기 인수 경계다.

## 보고서 그림 크게 보기(t-0552, plan §8.353)

- 웹 E2E `59-report-images`: 실제 host에 index.html·그림 넷(하나는 링크 안)·data: 그림을 publish한다. host 단계는 `node:http`로 보낸다(Node fetch는 `Sec-Fetch-Mode`를 cors로 덮어 브라우저 탐색 머리를 흉내 내지 못한다). fetch·Range·`?download=1`·그림은 원본 바이트, iframe 탐색은 원본 + `<script>` 하나와 맞는 `Content-Length`, sandbox CSP 그대로, 최상위 문서 탐색에도 붙는다.
- 데스크톱(Chromium): 보고서 안 그림 누름 → 크게 보기(`data-at`·`data-count`·`data-scale`·그림 주소), ←→·앞뒤 단추, 두 번 누름 2.5배·맞춤, 그림 누름은 닫지 않음, Esc·바깥 누름으로 닫고 보고서는 남음. 누른 곳이 iframe이라 키는 크게 보기가 focus를 가져온 사건(`focusIs`) 뒤에 누른다(앞서 누르면 키가 보고서 문서로 간다).
- 흉내 낸 알림: 보고서 제 script(API 주소·다른 origin의 같은 경로·data:)와 앱 창 자신(그 보고서 그림 주소)이 보낸 넷을 iframe 표지 `data-messages`로 본 뒤 `data-opened`·크게 보기가 그대로인지 단언한다. 진짜 누름 뒤에 보면 React가 한 번에 그려 흉내가 연 상태를 덮어 결함을 놓친다(처음 쓴 판이 출처 검사 결함에도 통과했다). 흉내 주소는 iframe의 실제 주소로 만든다(publish가 준 공유 링크 cap은 앱이 연 cap과 다르다).
- 휴대폰(WebKit iPhone): `tap()`으로 열고, 손짓은 touch PointerEvent를 무대·그림에 직접 보낸다(옆으로 쓸어 넘김, 짧은 끌기는 그대로, 두 손가락 90→190px = 2.11배, 남은 손가락 끌기는 옮기기이고 넘기지 않음, 두 번 누름 맞춤). 뒤로(popstate)는 크게 보기만 닫고 주소·보고서 그대로, 아래로 쓸기는 닫고 기록 칸(`aaa:sheet`)을 거둔다. headless WebKit은 이 iframe 안 그림을 그리지 않아(plan §8.353 「남은 것」) iframe 그림 읽힘은 데스크톱에서만 기다린다.
- 결함 심기(되돌렸다): iframe 출처 검사·origin 검사·`/v/<cap>/` 경로 검사를 빼면 데스크톱 흉내 단언, host가 fetch에도 붙이면 host 단계, 그림마다 빈 click listener를 빼면 휴대폰 첫 누름에서 실패한다.

## 「답할 것」 닫기·끝난 배포 접기(t-0559, plan §8.359)

- `node web/e2e/web.e2e.mjs --spec 26-activity-bar,35-rollout,39-phone-back,53-thread-questions,57-legacy-special-panes,62-asks-close`로 관련 화면 계약을 함께 확인한다. 26의 버튼 순서와 Home·End, 57의 패널 목록 단언은 사용자가 지정한 새 순서(작업부터, 답할 것은 보고서 바로 위)를 검사한다. 57의 옛 세션 숨김·실제 실행과 사용자 종료 기록·파일 바이트 보존·직접 읽기 검사는 그대로 유지한다.
- 62는 실제 격리 host·agent에 확인 필요 post, 셸과 Claude 대화 pane을 만든다. WebKit iPhone 390·430 × Safari 탭·홈 화면 앱 흉내와 Chromium 1280에서 질문 → 대화 → 뒤로 → 질문 → 뒤로 → 셸을 본다. 본문 위·가운데·아래 세 점의 맨 위 요소가 같은 화면에 속해야 하며 숨긴 「답할 것」은 visibility:hidden·position:absolute여야 한다. 수정 전 휴대폰은 두 화면으로 갈라졌고 데스크톱도 숨긴 화면이 공간을 차지했다.
- 닫기 단추는 앞서 본 대화로, 빈 「답할 것」에서 닫기는 앞서 본 보고서로 돌아가야 한다. 가장자리 합성 touchcancel 뒤 popstate를 내는 경우 목록이 열리지 않는지 검사한다(왼쪽 끝은 브라우저 뒤로 몫, plan §8.408). iOS 시스템 몸짓 자체는 헤드리스로 보증하지 않는다.
- 대기는 data-shown·aria-current·popstate·drawer transition 완료 사건으로 끝낸다. 숨긴 채 그려 둔 화면에도 drawer 단추가 남으므로 공용 inList는 보이는 화면의 단추를 고른다. 목록 행 클릭도 inList로 drawer 닫힘이 끝난 뒤 본문을 잰다.
- 35는 배포 실행기를 돌리지 않는다. 실제 Reader 세션의 WebSocket에 배포 상태만 주입해 WebKit 390·Chromium 1280에서 성공한 배포만 접기·펼치기, 높이 절반 미만 축소, 같은 배포 갱신·패널 전환 유지, 새 ID의 성공한 배포 재표시와 running·recovering·stopped·stopped_rolled_back·recovery_failed 상시 표시를 확인한다. 갱신 처리는 카드 data-updated로 기다린다. 기존 Reader 조작 거부·복구 재연결·긴 실패 원인 검사는 유지한다.

## 대화 그림의 task 첨부 전달(t-0558, plan §8.360)

- 웹 E2E23의 「대화 이미지」는 Chromium·WebKit 데스크톱·WebKit iPhone·iPad에서 붙여넣기·드롭·파일 선택으로 올린
  원본 바이트의 SHA-256 앞 16자와 저장 이름이 같은지 본다. iPhone 파일 선택은 MIME이 빈 `IMG_0001.PNG`도 보낸다.
  같은 그림을 task 업로드 API로 다시 보내면 같은 경로·파일 하나여야 한다. 각 기기의 대화 경로를 task 메모에 붙이고
  `thread start --from-task --link-task` 뒤 실제 작업 폴더의 첨부 바이트·task/brief의 사본 경로·보고를 확인한다.
- E2E23의 「옛 대화 첨부」는 무작위 이름의 fixture가 시작을 막으며 다시 첨부하라는 안내를 내는지, 원본을 다시 올려
  메모 링크를 바꾸면 thread에 같은 바이트가 전달되는지 검사한다. 거부와 복구 모두 옛 원본의 이름·내용을 보존해야 한다.
- m13 `task_attachments_reject_unverified_uploads_without_changing_them`은 옛 무작위 이름과 hash 이름의 원본을 실제로 바꾼
  경우를 각각 거부하고 원본을 보존하는지 본다. 기존 원격 TLS 전달·첫 사본으로 다시 시작 시험도 함께 실행한다.
- host `task_attach::tests::upload_rules`는 hash 이름의 기존 파일을 같은 길이의 다른 바이트로 바꾼 뒤 다시 올려도
  성공하거나 덮어쓰지 않는지 본다. 대화도 이 저장 함수를 쓰므로 두 화면의 보호 규칙이 같다.
- 수정 전 E2E23은 첫 Chrome 붙여넣기의 무작위 이름에서 실패했다. 시간 상한·재시도·해시 검사를 완화하지 않는다.

## 대화 첨부 미리보기 복원(t-0583)

- E2E23의 「대화 이미지」는 Chromium·WebKit 데스크톱·WebKit iPhone·iPad에서 붙여넣기 뒤
  대화 → 터미널 → 대화 → 보고서 → 대화로 이동하고, 드롭·파일 선택 뒤에는 새로고침한다.
  초안의 글·경로 보존, 복원한 썸네일 하나의 실제 이미지 decode, 전송 뒤 원본 SHA-256·경로와 미리보기 제거를 본다.
- 복원한 그림 뒤 새 그림을 붙여 하나씩 삭제하고, 새로고침 뒤 지운 경로·썸네일이 돌아오지 않는지도 확인한다.
  blob URL을 저장하지 않고 초안의 경로를 기존 pane_file 권한 검사로 읽는다.
- 원래 제품에서는 첫 Chrome 대화 → 터미널 → 대화 뒤 경로는 남고 썸네일이 없어 decode 대기에서 실패했다.
  시한·재시도를 바꾸지 않고 DOM 표시·입력 준비·이미지 decode·실제 입력 수신 사건으로 끝낸다.

## 입력칸 줄바꿈 폭 고정(t-0563, plan §8.364)

- 사용자 1A에 따라 글이 길어져도 그림·맥락·보내기 단추를 옆에 둔다. 한 줄/여러 줄 표지는 실제 글 칸의 줄 수이며 배치를 바꾸지 않는다. E2E 22의 단추 아래 줄 조건과 펼친 폭 표지만 이 선택에 맞춰 바꿨다. 짧은 글 높이·단추만 있는 밑 줄 금지·10줄 상한·focus·아래끝·잘림 검사는 유지하고 모든 길이의 글 칸 폭 불변 검사를 더했다.
- 새 `66-composer-wrap`은 실제 host·agent의 Claude pane에서 Chromium·WebKit × 데스크톱·iPhone 네 조합을 돈다. 한글 168음절의 자모 변화·음절 확정 사건을 재생해 매 조합 입력 뒤 크기를 기록한다. 같은 계산 CSS와 폭의 독립 native textarea로 줄 수와 첫 줄이 넘는 위치를 대조한다. OS 입력기를 직접 조작하는 시험은 아니다.
- 영문 단어·긴 영문 토큰·줄바꿈이 있는 붙여넣기·창 축소/휴대폰 회전과 복원·지우기를 검사한다. plain text paste 사건을 제품 handler에 보내고 취소되지 않으면 브라우저 편집 명령으로 기본 삽입을 실행한다. 시스템 clipboard는 건드리지 않는다. 창 변경은 ResizeObserver 사건으로 끝내고 고정 sleep·재시도는 없다. 편집 중 pane input·submit·bin 전송이 없어야 한다.
- `AAA_E2E_SHOTS`에는 같은 입력의 단계별 PNG와 `wrap-geometry.json`이 남는다. 변경 전 네 조합 모두 실제 글 칸 폭과 첫 줄 경계가 바뀌어 RED였다. 변경 후에는 조합 입력 336회씩 폭과 첫 줄 경계가 일정하고 native 줄 수와 일치했다. E2E 35의 전송 중 높이와 51의 지연 커서 keeper도 통과했다.
- 초기 머신의 `cargo build -p aaa --locked`는 빌드 스크립트 SIGKILL로 종료됐고 같은 시각 AMFI 서명 실패와 syspolicyd의 파일 열기 한도 오류를 기록했다. 사용자 지시로 시스템 서비스를 바꾸지 않고 다른 머신의 새 clone·target으로 검증을 옮겼다. 구현 후보 a5100fbb의 정본 ci-local 전체 7단계(웹 유닛 403개·Rust 863개·Python 8개·웹 E2E 65개 파일 전체)와 별도 공개 검사가 통과했다. 기존 수동 ignored 3개는 정본 설정 그대로다. 후보 내장 웹을 시험하며 초기 진단용 Node loader는 쓰지 않는다. 착지 때 main 통합 후보도 새 실험 폴더에서 ci-local 전체와 실제 이름 목록을 적용한 check-public을 요구한다.
- main의 `63-deploy`·`64-automation-failure`·`65-account-quota`와 겹치지 않게 `66-composer-wrap`으로 옮겼다. FRONT_API·AGENT_LINK나 기존 다른 E2E의 번호·시한·단언은 바꾸지 않는다.
- 통합 후보의 첫 전체 실행에서 E2E 51 준비가 실패했다. 입력칸은 그려졌지만 새 pane의 WebSocket이 아직 readonly를 해제하기 전이었고 가짜 agent의 수신은 0바이트·대화 기록도 비어 있었다. 준비에서 첫 Enter 전에 제품 보내기 단추의 disabled 해제 사건을 기다린다. 입력은 한 번만 보내며 답과 38% 게이지·지연 커서 단언·20초 상한은 유지한다. 연결 전 Enter가 글을 두고 보내지 않는 제품 계약은 기존 E2E 08이 계속 검사한다.
- 수정 후보에서 51은 통과했으나 E2E 25의 빠른 pane 전환도 같은 준비 누락을 드러냈다. Codex 수신은 앞 단계의 6바이트 그대로였고 새 입력의 submit은 없었으며 키 입력 뒤에 새 snapshot·readonly 통지가 왔다. 터미널은 현재 pane 이름·data-readonly=false, 대화는 기존 23처럼 그림 첨부 단추의 disabled 해제 사건을 기다린다. 두 대화 진입 helper를 함께 정리하며 Ctrl+Enter 키·기대 바이트·host 판정·시한은 유지한다.
- 이어진 전체 실행의 E2E 61은 safety 충돌 뒤 새 판 읽기가 끝나기 전에 입력을 다시 여는 제품 경합을 잡았다. SafetyPage는 재조회 Promise가 끝날 때까지 busy를 유지한다. 기존 충돌 시험에서 실제 safety_show 답만 gate로 보류해 그동안 선택이 꺼져 있는지 검사하고, 해제 뒤 원래 user 값 저장·사용자 주석 보존을 확인한다. 재조회 대기를 제거하면 새 판을 읽기 전 입력 금지 단언이 실패해야 한다. host의 판 대조·권한·기대값·시한은 바꾸지 않는다.

## 질문 카드와 확인 필요 post 묶기(t-0565)

- `node --test web/test/asksModel.test.mjs`는 제목의 `aaa:question/<thread>`가 열린 카드와 일치할 때만 post 여러 개를
  질문 한 줄로 묶는지 본다. 답한 카드의 늦은 알림도 묶는다. 카드 없음·보고서 참조·본문에만 참조가 있는 post는 그대로 남긴다.
- `cargo test -p aaa-host --lib asks::tests --locked`는 카드 대신 post를 처리하는 API 거부, 답 전송 경로의 거부 뒤
  질문·post 보존, 연결 post만 처리, 전송 중 새 알림 보존, `request handled`와 답한 질문 하나를 검사한다.
  `post handled`는 원래 발신 ID·화면 p-번호로 같은 항목을 처리하고 반복해도 한 번 기록한다. `inbox done`도
  post 목록·status의 처리 상태에 반영돼야 한다. 둘 다 질문 답·선택·승인을 만들지 않는다.
  옛 버전이 남긴 post는 새 보고로 질문이 닫힌 시각이나 성공한 답 기록으로 정리한다. 임시 답 claim은 성공으로 보지 않는다. 끝난 카드에 늦게 도착한 알림은 즉시 닫고 다음 질문의 알림은 유지한다.
- `node web/e2e/web.e2e.mjs --spec 53-thread-questions`는 실제 host·agent에 질문 카드와 연결 post 두 개를 만든다.
  수정 전은 빨강 배지 1개 검사에서 실패한다. Chromium 데스크톱, WebKit iPhone 390 Safari·430 홈 화면 앱에서
  한 줄·한 배지, 카드 답 뒤 post 둘의 inbox done과 `request handled`, 「답한 것」 한 줄을 확인한다.
  실제 coordinator post에만 있는 C는 `thread answer --text`로 원문을 전하고 A/B 선택지 수·선택하지 않은 picks를 보존한다.
  `post handled`만으로 카드 답이 생기면 실패한다. 새 보고로 질문이 닫히면 post도 닫히고 답 없는 닫힘 기록을 남겨야 한다.
  원문·그림 보존·새 질문·보기 권한·독립 post의 웹 처리·다른 기기에서 기록 지우기도 기존대로 검사한다.
- `cargo test -p aaa --test m13 worker_session_cli_is_read_only --locked`는 실제 worker pane의 새 `post handled` 명령도
  다른 쓰기와 같이 거부하는지 본다. 새 Front 요청은 조작 권한이 필요하고 승인·배포 요청과 연결하지 않는다.
- 외부 푸시의 참조 해석은 그대로다. `cargo test -p aaa-host --lib post::tests --locked`의 기존 사용자 post 시험이
  coordinator 검증·푸시 큐 유지·제목의 참조 제거와 `/#question=<thread>` 목적지를 확인한다.
  iOS 실제 푸시 수신과 시스템 Safari 자체는 헤드리스 시험이 보증하지 않는다.

## task 예약·완료 보관(t-0564)

- host `task_queue::tests`는 ID의 원문/CRLF 보존·편집 후 유지·중복 거부, coordinator 선택 변경 거부와 사용자 직접 변경,
  12개의 동시 DB 선점에서 6자리 상한·resolve_pending 보유, 내용 변경 뒤 확인 필요·취소의 재개방 보존을 검사한다.
  완료 보관은 실제 SQLite INSERT 실패 trigger에서 활성 파일·미판정 상태가 보존되는지, 정상 저장 뒤 같은 ID·본문·결과·첨부,
  이력 prune 뒤 보관 보존·같은 ID 복원·기동 연결 해제·재예약 없음도 확인한다. 완료 결과와 메모의 보고서 파일은 실제 기한 정리/삭제에서 보호되고, 관련 없는 사본은 정리되는지도 검사한다. 고정 sleep은 없다.
- m13 `task_queue_runs_possible_fifo_after_resolve_without_coordinator`는 실제 host·agent·CLI·PTY로 6개의 자리를 채우고,
  보고 [완료]·host 재시작에도 세 예약의 새 기동이 없는지 본다. 가장 늦게 등록한 예약을 CLI `queue-move`로 1번에 옮기고,
  저장된 차례·생성 시각 보존과 재시작 뒤 차례를 확인한다. resolve 완료 사건 뒤 coordinator/웹/수동 tick 없이 옮긴 1번의
  실제 pane 기동과 다른 예약의 대기를 확인하고, 그 thread도 resolve한 뒤 고정 머신이 끊긴 앞 예약을 남기고 가능한 뒤 예약의
  실제 pane·같은 task ID 연결을 기다린다.
- host `slot_released_during_queue_scan_does_not_bypass_the_moved_front`는 앞 예약의 `자리 대기` DB 기록 직후
  다른 thread가 자리를 반환하는 경합을 임시 SQLite trigger로 고정한다. 같은 순회의 뒤 예약이 머신 선택·기동으로 넘어가면
  실패하며, 다음 순회에는 앞에서부터 새 자리를 판정한다. 원래 코드는 뒤 예약만 머신 가용성 판정으로 넘어가 실패한다.
  기동 중 예약의 상태 대조·내용 검사는 계속하며, 끊긴 머신 등 시작 조건이 막힌 앞 예약을 건너뛰는 계약은 유지한다.
- 기존 m13 `parallel_threads_enforce_capacity_before_launch`는 한도 초과 직접 요청의 실패·thread 수·pane 수를 함께 본다.
  `task_thread_link_and_auto_clean`은 기존 확인 필요·부모 보호·옛 delete restore를 유지하며 완료 보관의 ID·본문·보고서·복원을 관측한다.
- 웹은 실제 host의 예약 상태와 완료 보관만 읽는다. 실패의 상한·재시도·기대값을 바꿔 통과시키지 않는다.
- 추가 source owner는 기본 모델 칸이 비어 있어도 승인된 선택을 coordinator가 바꾸지 못하는지, 직접 파일 수정 뒤
  coordinator의 재예약이 거부되는지, 사용자 API의 선택 변경만 새 출처로 기록되고 명시 갱신 전에는 과거 승인으로 실행되지 않는지 본다.
  기동 전 중단의 자리 회수/옛 송신 불명 보존·실패 예약 취소 후 tick 무변경·파일 제거 뒤 DB 확정 전 중단의 복구도 사건으로 확인한다.
- 웹 E2E `67-task-queue`는 데스크톱 예약 확인·두 기기의 같은 차례·휴대폰 맨 앞으로/취소·새로고침 보존·resolve 뒤 실제 기동·
  완료 보관/같은 ID 복원을 본다. 기존 47은 한도 우회 단추 부재와 완료 보관의 본문/결과/ID를 검사한다.
- 시작/재시작은 같은 기동 오류 판정을 쓴다. 송신 결과 불명은 Starting으로 자리를 유지하고, 확실한 새 기동 실패는 원인과 함께
  Failed로 남긴다. 이미 가진 Open 자리를 재시작할 때는 기동 전 중단에도 그 원래 자리를 유지하며 중복 선점하지 않는다.
- 완료 보관을 복원한 뒤 제목을 바꾸고 다시 완료하는 경우도 검사한다. 일반 이력이 정리된 뒤에도 이전 완료의 본문·thread와 그 결과물 참조는 보관 이력으로 이어진다.

## 요약 전체 작업 진행상황(t-0574, 사용자 1A)

- `cargo test -p aaa-host --lib summary:: --locked`: 완료·진행·대기가 섞인 다섯 작업의 순서, 대문자 체크, 잘못된 표지·빈 이름·여섯 항목·30자 경계와 옛 요약의 빈 진행 목록을 확인한다.
- `cargo test -p aaa-proto --locked`: 옛 host 응답의 누락된 `progress`는 빈 배열이고, 기존 now·done은 보존됨을 확인한다. Rust 정본과 생성 TypeScript도 검사한다.
- 기존 m13의 대화·확인창 흐름은 실제 CLI의 요약 쓰기/읽기로 순서·세 상태의 JSON을 확인하고, 새 지시를 보내도 저장된 목록이 바뀌지 않음을 확인한다.
- `node web/e2e/web.e2e.mjs --spec 28-thread-summary`: 기존 옛 요약·hover 위치·닫힘·고정 줄·휴대폰 표시를 그대로 실행한다. 새 단계는 실제 host·agent·CLI가 쓴 다섯 작업을 hover에서 모두 읽고, 열린 카드 중 요약 갱신→새 완료 개수와 상태→새 지시 경고의 DOM 사건을 기다린다. 전체 완료와 목록 없는 요약으로 바꾸어 열린 상세의 캐시가 바뀌는지도 본다.
- 같은 E2E의 실제 Reader 기기로 Chromium 320px·WebKit 390px 상세를 확인한다. 긴 이름은 hover에서 한 줄 생략, 휴대폰에서는 줄바꿈한다. 시트 움직임은 `getAnimations().finished`로 끝을 기다리고 가로 넘침을 잰다. Reader의 직접 쓰기 요청도 거부하고 원본을 보존해야 한다. 고정 sleep·시한 연장·상태 추측은 없다.

### 착지 검증의 파일 링크 좌표(t-0574)

E2E 16은 첫 snapshot의 글이 보인 직후 링크 좌표를 재면 뒤늦은 초기 resize와 경주할 수 있다.
xterm은 buffer resize에서 현재 링크를 지우고 다음 mousemove에서 다시 찾는다. 실제 pane resize 요청을
소켓 gate로 보류한 진단에서 `xterm-cursor-pointer`가 resize 뒤 사라지고 새 마우스 이동 뒤 돌아옴을 확인했다.
기존 실패는 출력이 모두 있는 상태에서 `src/long.ts`의 링크 커서만 20초 안에 오지 않았다.

`clickLink`는 이 화면의 `data-fit="same"`·pending 없음과 실제 그린 행 수까지 맞춘 뒤 좌표를 잰다.
원래의 커서·클릭·문법 강조·매체·폴더·설정·가상화 단언과 20초 시한은 유지한다. 실패 재실행이나
시한 연장으로 넘기지 않으며, 진단과 좁은 E2E 16 뒤 수정한 통합 후보의 전체 ci-local을 실행한다.

## coordinator 질문 카드·짧은 질문 ID(t-0576, 1단계)

- `cargo test -p aaa-host --lib question_ids::`: 프로젝트별 q-1000 시작, 같은 대상의 동시 발급,
  다음 보고 판의 새 번호, DB 재개, 답한 것 삭제 뒤 순번 보존을 검사한다. 구조화 post는 본문 hash·선택지 검증,
  ID별 부분 답과 긴 자유 답의 원문·선택 보존·library 사본, 두 기기의 동시 제출 한 건, 평문 post의 ID 처리 표시를 본다.
  완료한 부분 답 초안은 비우고 ID 원장만 남긴다. DB 종료의 `done=0` 조건을 제거한 변형은 중복 답 단언에서 실패해야 한다.
  변형 원본 bytes를 복원한 뒤 정본 전체를 검증한다.
- `cargo test -p aaa-host --lib short_question_ids_reach_push_titles`: push 제목의 q-ID,
  기존 thread 링크와 새 q-ID 링크, 앱 참조가 알림 문구에 남지 않는 것을 검사한다.
  기존 post privacy keeper는 새로 추가된 ID까지 정확히 비교하며 제목의 경로·token 가림과 평문 기본 링크를 유지한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 69-coordinator-questions`: 실제 host·agent·CLI를 쓴다.
  두 종류의 카드는 같은 QuestionCard에서 렌더링하며 질문 DOM 구조를 비교한다. 데스크톱·WebKit 휴대폰 각각
  밝음·어두움 캡처, Markdown 표·그림, 추천·선택·직접 답, 다른 기기의 답 보존, Reader의 두 답 경로 거부,
  `thread answer q-ID A`·`question answer q-ID B`·`post handled q-ID`, 평문 호환을 본다.
  닫히거나 지워져 찾을 수 없는 q-ID를 열 때는 다른 질문을 대신 열지 않는다.
  그림은 실험실의 SVG와 publish 사본뿐이다. 이미지 decode·DOM 표지·host 상태로 끝나며 고정 sleep을 쓰지 않는다.
- 기존 E2E 53은 thread 질문 알림 여러 건을 한 카드로 묶기, thread로 답 전달, 늦은 알림·다음 보고,
  평문 post와 보존·삭제를 계속 검사한다. `web/test/asksModel.test.mjs`는 q-ID 링크와 기존 thread 참조의 같은 카드 연결을 본다.

전체 정본은 `bash scripts/ci-local.sh`이며 공개 검사는 새 파일을 추적한 상태에서 `bash scripts/check-public.sh`로 확인한다.
운영 host·agent·설정·인증에는 닿지 않는다. 보고서 r-ID는 별도 2단계다.

### 페이지 종료 뒤 예약 조회 재개(t-0584 착지, 사용자 1A)

E2E23의 「재로딩 중 예약된 자동화 조회」는 WebKit의 첫 `projects` 요청을 route gate로 붙잡고 events 연결·첫 snapshot의
갱신을 합친 뒤 reload한다. 종료로 첫 요청이 취소된 뒤 `finally`가 예약된 조회를 다시 보내던 원래 제품은
`Fetch API cannot load … due to access control checks` 페이지 오류에서 실패한다. `pagehide`에서 후속 조회를 막는 제품은
오류 없이 새 문서를 연다. WebKit의 요청 취소는 `pagehide` 전에 올 수 있어, 실패한 읽기의 `catch`도 예약을 비워야 한다.
종료한 문서의 갱신 사건은 fetch를 보내지 않고, `pageshow`는 실제 새 `projects` 요청으로 끝난다.
route gate는 service worker를 차단한 실험실에서만 쓴다. 고정 sleep·시한 연장·페이지 오류 무시는 없다.
기존 E2E23의 그림 전송·복원·해제·권한·Chrome/Safari/iPhone/iPad 검사는 그대로 함께 돈다.

## 머신 상세(t-0578 웹 머신 추가 M0, plan §8.372)

- host 유닛 `cargo test -p aaa-host machine_view_tests`: 실제 host DB를 연 실험실에서 hello(CPU 종류·판·SHA·기능·시작 점검)가 `MachineView`에 실리는지, 다른 판으로 다시 붙으면 `software_differs`, 모르는 꼴의 점검은 버리는지 본다. runtime을 끝내 Host가 닫힌 것을 확인한 뒤 같은 상태 폴더로 다시 열어 판·SHA·CPU 종류는 DB에서 남고 기능·점검은 빈 것을 본다.
- 웹 유닛 `node --test web/test/machineDetail.test.mjs`: 줄 요약(옛 host는 OS만), 판 주의·SHA 고정폭·「모름」, 연결·끊김의 마지막 본 때, 건강은 연결 중일 때만, 시작 점검.
- 웹 E2E `60-admin`의 「머신 상세(M0)」: 등록해 붙인 둘째 agent의 `aaa machine list` SHA가 실행 파일(`AAA_BIN`)의 SHA-256과 같고, 줄 요약·「자세히」의 SHA·판·연결·기능 목록이 그 값을 보인다. 보기 기기도 「자세히」를 열고, 휴대폰 390은 상세를 펼친 채 가로 넘침이 없다. 기다림은 화면 표지와 host 답이다.

## aaa install I6 승인·실제 executor owner(t-0485, plan §8.374)

- `cargo test -p aaa --test install --locked`: 공통 실제 CLI/PTY owner는 세 OS에서 같은 마법사·plain/inquire·뒤로·초안·정확 yes·EOF130·자동/세션 거부·private 발급·실제 worker·같은 ID status/resume를 검사한다. 실제 TLS host 등록/SAS/응답 유실 조회·외부 pin 편집 보존, require unknown→새 defer 승인/파일 job 재사용, 첫 Controller 실제 redeem/폐기·코드 비공개를 포함한다. 운영 서비스/계정/암호/LSA는 사용하지 않는다.
- `cargo test -p aaa --bin aaa install:: --locked`: 실제 private 파일·서명/pin/만료/nonce·파일 scope·execution/job 결속·I2/SCM 상태·보호 입력·PATH 정책을 검사한다. 독립 v1 status/exit validator가 needs_user3와 cancelled130을 소유한다. 논리 clock은 승인 만료에만 쓰고 실제 쓰기는 OS 파일 transaction이다.
- `cargo test -p aaa-host --lib installation:: --locked`, `cargo test -p aaa-host --lib tailscale:: --locked`: 실제 host DB/서명/private 입력과 TLS peer·grant 일회 소비/폐기, 보호 publication의 PID/image/nonce/phase/실제 owned listener, exact 포트/외부 mapping 보존, bounded LocalAPI wire의 ETag/If-Match/412, 현재 host HTTPS proof와 일반 200 거부를 검사한다. publication 소비자의 입력 fixture는 파일 엔진 생산 완료 증거로 쓰지 않으며 실제 CLI owner가 생산 경계를 검사한다.
- `cargo test -p aaa-host --lib install_agent_query_keeps_the_handshake_key_after_same_id_rekey --locked`: 첫 메시지 전 key 교체 때 실제 연결의 immutable peer를 유지하고 다른 key의 결과를 내주지 않는다.
- Windows 전용 owner는 실제 ConPTY 암호 mode/echo/성공·취소·EOF, Job의 breakaway 허용/금지와 부모 소멸, Win32 ready 파일 변경 사건, 임시 HKCU Software lab key의 registry transaction rollback/동시 편집이다. Windows API 차이 때문에 cfg(windows)이며 공통 CLI/권한 owner는 제외하지 않는다. Unix 부모 감시는 macOS kqueue/Linux pidfd를 사용한다. SSH 신뢰 unit은 봉인된 경계만 실험실에서 대체하며 실제 OpenSSH 설치 실행 성공을 주장하지 않는다.
- 서명·계획/소비·파일 scope/job 연결·worker 분리 수명·정확 Serve의 If-Match/선택 포트·HTTPS instance proof·TLS rekey/소비/PATH 보존 변형을 owner에서 RED로 대조하고 바이트 복구 뒤 검증한다. 실패 재실행·timeout 증가·기대값 완화는 하지 않는다.

Windows/Linux 실제 실행 판정은 main push 뒤 GitHub CI windows-2025/ubuntu-24.04다. 실제 SCM logon/LSA/Session0/UAC/재부팅/실제 서비스 harness 인증·Tailscale HTTPS는 사용자 Windows PC 사용자 인수로 따로 확인한다. 교차 clippy/build와 file/PTY owner를 이 실제 설치 증거로 부르지 않는다.

## 자동화 thread의 별도 실행 자리(t-0586)

- host `task_queue::tests::automation_threads_have_atomic_separate_slots_and_restart_limits`는 Barrier 뒤 12개 DB 선점을 경주시켜 사용자 자리 1개·자동화 규칙 자리 1개만 허용한다. 같은 actor인 사용자도 세고 정리 중 자동화·실패 재시작·상한 초과 재시작·자리 반환을 확인한다.
- host `automation::start_tests`는 다음 사건을 기본 동시 1개에서 내구 대기하고 명시 2개로 넓힌 뒤 처리하는지, 같은 사건 영수증·0의 기본 상한·DB 최종 선점·기존 holds를 검사한다. 기존 reuse keeper는 열린 thread로 이어 보내기·기억·완료 뒤 쉼을 그대로 검사한다.
- m13 `automation_start::automation_threads_leave_user_capacity_and_queue_slots_available`은 실제 host·agent·CLI로 사용자 한도가 차도 자동화를 시작하고, 사용자 직접 시작은 막으며, 사용자 resolve 뒤 자동화가 열린 채 다음 예약이 실제 pane으로 시작하는지 확인한다. CLI list/show/context 표지·출처, host 재시작 뒤 보존·자동화 재시작도 본다. 기존 자기 사건 거르기 keeper는 명시 상한 2개로 두 원인을 비교한다.
- 웹 E2E `48-automation-threads`는 기존 실제 규칙 기동·막대·규칙 이동을 유지하며 데스크톱·휴대폰에 같은 「자동화 thread」 표지, 모든 thread 목록, thread 상세의 규칙/일정/자리 제외 설명을 검사한다. 보고·DOM·기동·연결 사건으로 끝내며 고정 sleep·시한 연장은 없다.
- 일반 자리에서 자동화 제외를 제거한 변형과 규칙 상한을 제거한 변형이 새 DB keeper를 실패시키는지 확인한 뒤 원본 bytes를 복구한다. 정본 전체 검증은 `bash scripts/ci-local.sh`다.

착지 통합 후보는 t-0584의 `slot_released_during_queue_scan_does_not_bypass_the_moved_front`를 자동화 thread 없음/열림 두 경우에 실행한다. 사용자 자리가 순회 도중 반환돼도 뒤 예약은 추월하지 않고, 다음 순회에서는 자동화 thread를 자리에 넣지 않아 앞 차례부터 조건을 다시 확인해야 한다.

## 「직접 설치」 안내(t-0578 웹 머신 추가 M1, plan §8.383)

- host 유닛 `cargo test -p aaa-host enroll_guide`: 연 주소(모든 주소·tailnet 주소·LAN·loopback·이름)별 주소 후보와 순서.
- 웹 유닛 `node --test web/test/enrollGuide.test.mjs`: 1.92 판정, 이름·주소 글자 규칙, 명령에 주소·이름·판·지문, 쓸 수 없는 값은 자리표시.
- 웹 E2E `60-admin` 첫 단계: 안내를 펼쳐 실험실 host가 연 loopback 주소가 후보로 오고 「host 머신 안에서만」 경고, 이미 있는 이름(labA) 알림, 둘째 단계 명령이 `~/.local/bin/aaa agent enroll <주소> --name labB`인지 본 뒤 실행 파일 자리만 실험실 binary로 바꿔 그대로 실행해 등록한다. 안내가 보인 host fingerprint 앞 16자가 새 머신 `agent enroll` 화면의 것과 같아야 한다. Reader에는 안내가 없고, 휴대폰 390은 안내·상세를 펼친 채 가로 넘침이 없다.

## SSH로 머신 추가(t-0578 웹 머신 추가 M2, plan §8.384)

- 실험실 sshd: 시험이 임시 폴더에 host key·사용자 키·`authorized_keys`·`sshd_config`(loopback 빈 포트, 비밀번호·PAM 끔, `StrictModes no`)를 만들고 `sshd -f <config> -D -e`를 자식으로 띄운다. stderr의 `Server listening` 줄이 나오면 시작한 것이다. host는 `[host] ssh_config`의 실험실 설정(별칭 `lab`, 닫힌 포트 `lab-down`, `IdentitiesOnly`)만 쓴다. 같은 사용자 로그인뿐이지만 「자기 자신을 머신으로」 접속하기에 충분하다. 시험이 띄운 sshd 자식만 끝낸다. sshd가 없으면 건너뛰고 그렇게 적는다.
- host 유닛 `cargo test -p aaa-host machine_setup`: 순수 규칙과 실험실 끝까지(처음 키 → 세션 안·Reader·다른 지문 거부 → 믿음 → aaa known_hosts 한 줄 → 감지 = 이 빌드의 OS·CPU → 다시 하면 아는 키 → 닫힌 포트 까닭 → known_hosts를 다른 키로 바꾸면 changed로 멈춤 → Reader 목록·지우기). 단계 대기는 host `enroll` 사건이다.
- 웹 유닛 `node --test web/test/machineSetupModel.test.mjs`, 웹 E2E `72-machine-add`(데스크톱: 비밀번호 꼴 막음, 지문이 `ssh-keygen -lf`와 같음, 믿음 확인 창, 준비됨·감지 줄, aaa known_hosts 한 줄, 닫힌 포트 까닭 / 보기 기기: 조작 없음 / 휴대폰 390: 폭·지우기).
- Windows: `windows_runner_reports_whether_an_openssh_server_exists`가 CI 로그에 sshd.exe 유무만 적는다(실패하지 않는다).

## 병합 대기·병합 중 실행 자리(t-0594, plan §8.385)

- `cargo test -p aaa-host --lib report_mark::tests --locked`: 첫 줄·공백·BOM/CRLF·중복/붙임 표지·본문·완료/다음 단계 경계.
- `cargo test -p aaa-host --lib report_stages --locked`: 같은 SQLite의 여덟 동시 요청이 마지막 한 자리만 선점하는지, 병합 대기·착지 한 자리만 제외하며 자동화의 착지는 그 한 자리를 빼앗지 않는지, 착지 실패 수리·작업 복귀 초과·정리 중/완료를 검사한다. 옛 JSON과 실제 보고 사본으로 Host를 다시 열어 marked 보고는 이행하고 unmarked/완료·hash 불일치·유실은 보수적으로 세는지도 확인한다. 운영 폴더는 읽거나 쓰지 않는다.
- `cargo test -p aaa --test m13 thread_stages:: --locked`: 격리 host·agent·CLI·가짜 harness에서 한도가 찬 상태의 예약 두 건을 순서대로 준비한다. 병합 대기 보고 회수 사건이 첫 예약을, 그 예약의 착지 보고가 둘째 예약을 시작해야 한다. 단계를 알기 위해 ticker를 반복 호출하지 않는다. CLI 세 출력·예약 DTO·ack 보존·다음 단계 복귀 시 새 기동 거부/기존 작업 보존·host 재시작 뒤 같은 단계를 검사한다.
- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 67 --workers 1`: 기존 예약 순서·취소·다른 기기·완료 보관을 유지하고, 실제 보고 파일→host 단계→예약 기동→데스크톱/390px 할 일의 연결·미연결 표지와 다른 색·사이드바·모든 thread·예약 상세를 확인한다. 스크린샷은 시험 산출물에 남긴다. DOM·보고·기동 사건으로 끝내며 고정 sleep·시한 연장은 없다.

병합 대기 제외를 제거한 제품 변형은 DB 선점 keeper가 실패해야 한다. 변형 뒤 제품 bytes를 복원하고 정본 `bash scripts/ci-local.sh` 전체와 공개 검사를 실행한다. 실제 동시 착지 명령을 강제로 중단하는 기능은 아니며 coordinator가 착지를 하나씩 시작한다.


## 보고 판 r-ID·실행 입력 이름(t-0576, 2단계)

- `cargo test -p aaa-host --lib report_ids`: 프로젝트별 동시 예약·재개·삭제 후 순번·실패 예약의 비노출, 옛 r 이름과 충돌 방지, 첫 기동 등록, q/r의 프로젝트 이름 변경 보존을 본다. 같은 이름을 두 번 발행하고 옛 r의 본문·그림·CLI 열람·공유 링크·삭제·Reader 거부를 본다. thread 두 판의 옛 Markdown·그림·inbox r 번호와 미복사 파일의 최신 agent 대체 금지도 본다.
- `cargo test -p aaa-host --lib library_links::`: 기존 서명·범위·권한·만료·기기 폐기·삭제·보존 규칙을 지킨다. host 용량은 불변 판까지 정확히 세며 live 원본의 나중 증가는 상한 밖, 등록 당시 사본은 보호 용량임을 구별한다.
- `automation::runtime::input_names_tests`: 옛 inflight와 새 run 이름을 직렬화·재개해 같은 processed 영수증 이름을 유지한다. E2E 49는 실제 자동화 시작의 run 파일(64자 상한)과 processed.txt를 확인한다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 73-report-ids`: 실제 host·agent·CLI의 같은 이름 재발행·thread 보고 변경, 이전 판 원문·그림, 프로젝트 없는 r hash·기존 이름·t 번호·판 선택·inbox·Reader 거부와 390/1280px 밝음·어두움 머리를 본다. 특정 hash의 thread 사건·DOM·이미지 decode를 기다리며 고정 sleep을 쓰지 않는다.


## 질문 안 자료 보기·기본 검수 카드(t-0576, 3단계)

- `node --experimental-strip-types --test web/test/questionMaterials.test.mjs`: 본문·목록·표의 같은 보고 r 링크를 한 번만 세고 그림을 중복 자료로 만들지 않는다. 음원·HTML은 안전한 질문 파일 경로로만 풀며 code·API·로컬 절대 경로·실행 scheme·경로 이탈은 거부한다.
- `cargo test -p aaa-host --lib report_mark::`: Questions 없는 검수 대기만 기본 카드로 만들며 명시한 질문을 보존하고 A를 추천하지 않는지 본다. report_ids keeper는 기존 검수 보고를 한 번만 q로 등록하고 B 글 누락 거부 뒤 답·부분 답을 선점하지 않음을 본다.
- `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 74-question-review`: 실제 publish·coordinator post·thread 보고를 쓴다. 카드 안 보고서·그림·음원과 자동 재생 없음·MIME·원래 질문 주소·390px WebKit/1280px 밝음·어두움, 선택/글 답과 inbox·새 보고 뒤 답한 r 판의 옛 그림을 확인한다. 소리 출력·운영 자료·고정 sleep을 쓰지 않는다.
- 보고 판 등록의 격리는 독립 keeper로 지킨다. 일반 Host::open에 사본 등록을 되넣으면 등록 전 비어 있어야 할 ID 단언이 실패하고, DB 소유 필터를 빼면 다른 state의 원문을 채택하지 말라는 단언이 실패해야 한다. 기본 경로의 프로젝트를 시험 fixture로 사용하지 않는다. 실제 host 기동의 실험실은 설정에 임시 projects root를 명시한다.

E2E 74의 headless WebKit iframe 그림은 기존 E2E 59·plan §8.353과 같은 이중 sandbox 제약을 적용한다. iframe의 그림 노드를 확인하고 같은 cap 주소의 실제 이미지를 앱 크게 보기에서 decode한다. Chromium iframe과 질문 본문의 그림은 직접 decode한다. 정책·시한을 바꾸거나 성공 여부를 추측하지 않는다. WebKit을 별도로 띄운 실험실은 실패 때도 finally에서 그 browser를 닫는다.

`report_ids::tests::archived_r_attachment_protects_its_entire_report_family_from_delete_and_prune`는 완료 보관의 옛 r 첨부가 같은 이름의 모든 판을 명시 삭제·기한 정리에서 보호하는지 검사한다. 기존 m13 library_view의 최신 t 별칭 전체/Range 스트리밍을 유지하면서, 같은 파일의 r 보기만 미복사 파일의 기존 보기 경로 오류(502)와 명시한 까닭을 확인한다. 최신 별칭과 불변 판의 읽기 계약을 구분한다.

전체 캡처를 켠 E2E 06은 설치 글꼴 검사에서 시작한 가짜 face를 아직 gate로 붙잡은 동안 screenshot이 document.fonts.ready를 기다려 막혔다. 보류 전의 크기·값·거부 단언은 그대로 두고, 캡처를 face gate 해제와 그리드 정합 확인 뒤로 옮긴다. 시한·기대값·제품 로딩 정책은 바꾸지 않는다.

착지 main 통합 뒤 ThreadView의 단계·자동화 출처·r 번호가 합쳐져 FrontEvent의 enum 크기 차이가 Clippy 상한을 넘었다. Thread 사건·응답 본체를 Box에 두어 JSON/생성 TypeScript 형식을 유지하고, 전체 WS/E2E·Rust 사건 수신 시험과 Clippy를 다시 검증한다.

## 배포 감시의 웹 응답·재시작 rollout(t-0538 관리 GUI W6c, plan §8.402)

- `cargo test -p aaa-agent --lib deploy::`: `web_probe`는 가짜 웹(TcpListener)으로 짝 없는 기기 답(401 `unauthorized`)만 답으로 보고 요청 줄·Host `127.0.0.1:<port>`를 확인한다. 안전 모드 503·421·JSON 아닌 401·잡음, 닫힌 port, 받고 답하지 않는 웹(한도 200ms로 줄여 사건으로 끝냄), 주소 아님은 답이 아니다. `0.0.0.0:<port>`는 loopback으로 묻는다. `guard`의 웹 판정은 실행 신원이 맞아도 웹 주소가 있으면 AwaitingNew·`pending_web`, 웹이 답한 뒤 NewReady·확정, 같은 한도에 복구(늦은 답은 복구를 되돌리지 않음), 주소 없음·빈 주소·agent 감시는 실행 신원만, 한도를 늘리지 않음을 순수 전이로 본다.
- `cargo test -p aaa-host --lib rollout::`: 웹은 개별 `deploy_start`(교체·재시작)를 모두 `forbidden`, coordinator 개별 재시작은 `bad_request`. 재시작 rollout은 끊긴 대상에서 아무것도 남기지 않고, 붙은 agent의 hello 실행 신원을 단계 baseline·rollout SHA·판으로 고정하며, 같은 ID(대상을 비우면 host 머신)는 같은 기록, 다른 서비스는 거부, 진행 중이면 새 재시작 거부, 두 머신 거부, 신원을 알리지 않는 agent는 `unsupported_version`, host 재시작은 host 자신의 신원. 가짜 agent(duplex link)가 감시 세우기에 답한다: 세운 영수증 뒤에 `guard_ready`, 설치 파일이 실행 판과 다르다는 `conflict`면 웹은 `conflict`이고 기록은 재시작 없이 중단, 같은 ID 동시 요청은 기록과 감시가 하나, 첫 세우기의 답이 끊기면 진행 루프가 같은 정본으로 다시 세우고 그때 `conflict`면 다른 요청(재시작) 없이 중단하며 그 감시를 취소(rollout 변경 알림으로 기다린다). agent `guard` 순수 전이: 재시작만 하는 감시(기대 판 = 배포 전 판)는 설치 파일이 바뀌어도 띄우기 전 취소·시한이 취소, 띄운 뒤 시한은 복구.
- `cargo test -p aaa --test m13 rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart`(macOS): 웹을 연 실험실. 첫 rollout의 새 host를 `AAA_TEST_WEB_HOLD_ME`(debug 빌드에서 `/api/v1/me`가 답하지 않음)로 띄우면 host 감시가 그 host의 웹 주소(`status`의 web_listen)를 `pending_web`로 두고 awaiting_new에 머문다 → 같은 판을 그대로 띄우면 이어 간다. 뒤의 모든 host 교체·복구도 웹 응답을 거친다. 끝에 coordinator `aaa service restart --role host`가 재시작 rollout을 만들고, 가짜 launchctl 기록 뒤 같은 실행 파일로 host를 다시 띄우면 done·감시 verified_new·실행 파일 그대로·재시작 한 번. 실험실은 agent port만 고정한다(웹은 시작마다 OS가 고른다).
- `cargo test -p aaa --test m13 coordinator_deployment_is_gated_and_survives_backend_restarts`(macOS): coordinator `aaa service restart --role agent --machine labB`는 재시작 rollout, 기록 뒤 같은 실행 파일로 agent를 다시 띄우면 done. 감독자 재시작 실패를 넣으면 작업을 다시 보내지 않고 감시의 복구 재시작 한 번 뒤 recovery_failed(오류에 주입한 실패). 이어 개별 update로 설치 파일을 바꾼 뒤 `service restart`는 stderr에 「감시 밖 개별 재시작」을 남기고 개별 재시작 작업(같은 ID, `update status --wait` completed, 재시작 기록 한 번)이며 그 ID의 rollout은 stopped(오류에 `aaa update status <ID>`).
- `node web/e2e/web.e2e.mjs --spec 63-deploy`: 「서비스 재시작」 확인 창(감시 안내) → 「배포 기록」에 「agent 재시작 ·」 카드 → 가짜 launchctl 기록. 옛 agent 그대로면 끝나지 않고, spec이 감독자 대신 agent를 다시 띄우면 done. 보기 전용 기기도 그 카드를 본다. 실험실 정리는 감시 프로세스를 명령줄의 `deployment-guard`와 실험실 상태 폴더로 확인한 PID만 끝낸다.

## 조작 기기 passkey(t-0538 관리 GUI W7, plan §8.410)

- `cargo test -p aaa-host --lib passkey::`: ring으로 만든 소프트웨어 인증기(ES256·EdDSA 키, 손으로 만든 COSE·attestation object CBOR)로 짝짓기 → 10분 안 등록 → 공개 주소의 승인(`web_pair`·다른 기기 폐기)을 끝까지 돌린다. loopback 화면은 passkey 상태가 없고 바로 승인, 공개 주소는 passkey 없으면 `passkey_required`(등록 안내), 읽기·제 기기 폐기는 묻지 않는다. challenge가 op ID와 요청 hash만 묶는지, 같은 서명을 다시 보내면 실행하지 않는지, UV 없음·다른 화면 주소·다른 RP·다른 challenge·남의 키 서명·모르는 credential은 거부하고 그 op도 버리는지, 다른 기기가 op를 마치지 못하는지 본다. 등록은 loopback·보기 기기·UV 없음·다른 credential ID·10분 지남(시작 뒤 지남 포함)을 거부하고, 다른 기기의 credential을 가져가지 못하며, 기기 폐기 때 지운다. CBOR는 길이 없는 값·잘린 값·남은 크기보다 큰 길이·너무 깊은 중첩을 거부한다.
- `cargo test -p aaa-host --lib web::guard_tests::only_a_direct_loopback_screen_approves_without_a_passkey`: 라우터 전체로 loopback(직접)은 코드가 나오고, 공개 Host와 serve를 거친 loopback Host(`X-Forwarded-Proto`·`X-Forwarded-For`)는 `passkey_required`.
- `node --test web/test/passkey.test.mjs`: base64url 왕복(채움 없음)과 등록 단추를 여는 규칙.
- `node web/e2e/web.e2e.mjs --spec 79-passkey`: debug host를 `AAA_TEST_WEB_LOCALHOST`로 띄워 `http://localhost:<port>`를 공개 주소처럼 쓴다(브라우저 passkey는 IP에 묶이지 못한다, RP ID `localhost`). Chromium 가상 인증기(CDP `WebAuthn`)를 단 휴대폰 화면이 짝지은 뒤 안내 줄로 등록하고, 설정 → 폰 및 기기에 등록됨·기기 목록 「passkey」 표지, 「기기 페어링」 코드가 서명 한 번(인증기 signCount)으로 나온다. 누름이 지난 뒤(`navigator.userActivation.isActive`를 거짓으로 고정)는 「passkey로 승인」 창을 한 번 더 누른 처리기에서 서명하고, 그 창을 취소하면 서명도 요청도 없다. 휴대폰 390에서 다른 확인 창(공유 링크 키 교체) 위의 그 창을 취소해도 아래 창과 안내가 남는다(그 창은 「뒤로」 기록을 쓰지 않는다). 인증기 없는 공개 주소 기기는 거부·등록 안내, 실험실 첫 화면(127.0.0.1)은 같은 동작을 passkey 없이 한다. UV 표지 없는 서명 거부는 host 유닛이 본다(확인하지 못한 가상 인증기는 브라우저가 제 창에서 기다린다).
- 결함 심기(library `w7-fault/README.md`): gate 열기·UV 검사 빼기·op 다시 쓰기·10분 시한 빼기·화면 주소 검사 빼기·서명 검사 빼기는 host 유닛이, 웹 `call`의 gate 빼기·웹의 UV 다시 보내기 빼기·「passkey로 승인」 창을 휴대폰 sheet(ConfirmDialog)로 되돌리기는 E2E 79가, serve를 거친 Host 판정 빼기는 guard 유닛이 잡았다.

## 「CLI 0회」 인수(t-0538 관리 GUI W8, plan §8.417)

- `node web/e2e/web.e2e.mjs --spec 82-cli-zero`: 설치(프로젝트 만들기, 첫 짝짓기) 뒤의 운영 한 바퀴를 웹 화면만으로 돈다. 새 머신 등록 → 설정 「안전」 `start_threads = auto` → 자동화 ①(사건마다 새 thread: 폼 → 시작 범위 승인 → 지금 실행) → 자동화 ②(열린 thread에 글: ①의 thread 번호 → 지금 실행 → 그 thread 터미널에 글) → 배포 점검·시작(대상 dry-run에서 멈춤)과 agent 재시작 rollout(가짜 launchctl) → agent·host를 죽이고 다시 띄우면 화면이 끊김을 말하고 저절로 돌아옴 → 깨진 host.toml의 안전 모드와 되돌리기 → thread 끝내기·자동화 지우기·머신 폐기. 설치 뒤 spec의 `cli()`는 보기 명령만 통과시키고 다른 aaa 명령은 「CLI 0회 위반」으로 멈춘다(첫 단계가 그 거부를 직접 본다). agent 연결 주소는 고정 port다: 다시 띄운 host에 새 머신이 다시 붙는다.
- 실제 머신 점검표(사용자가 웹과 화면 공유만으로, 터미널 입력이 한 번이라도 있으면 미완료):
  1. 휴대폰을 공개 주소로 다시 짝짓고 10분 안에 passkey를 등록한다. 이 뒤 승인 필요 동작은 passkey 확인 한 번이다.
  2. 새 머신을 더한다: 설정 → 머신 → 등록 창의 안내 명령을 새 머신에서 치고(그 머신의 설치), 확인 코드로 승인한다. 「연결됨」을 본다.
  3. 설정 → 안전에서 `start_threads`를 `auto`로 바꾼다.
  4. 자동화 ①: 새 자동화 → 「새 thread 시작」(profile·머신) → 저장 → 「승인 검토」 → 승인 → 「지금 실행」. 작업 목록에 자동화 thread가 서고 보고가 온다.
  5. 자동화 ②: 새 자동화 → 「thread에 글」(4의 thread 번호) → 저장 → 「지금 실행」. 그 thread 터미널에 글이 온다.
  6. 설정 → 배포: 빌드 머신에 있는 후보(coordinator나 thread가 빌드해 둔 것)를 점검하고 배포를 시작해 기록에서 끝까지 본다. 「agent 재시작…」으로 한 머신 agent를 다시 띄운다.
  7. 장애: 화면 공유로 agent 머신의 aaa agent 프로세스를 활동 모니터에서 끝내고, 웹의 끊김 표시와 감독자가 다시 띄운 뒤의 복귀를 본다. host Mac에서도 같게 한다.
  8. 정리: 4의 thread를 「thread 관리 → 끝내기」, 두 자동화를 지우고, 2의 머신을 폐기한다.
  9. 화면 공유 밖에서 터미널을 연 일이 있으면 그 단계와 까닭을 적는다(미완료).
- `node web/e2e/web.e2e.mjs --spec 83-cli-zero-gaps`: 인벤토리의 작은 빈칸 다섯(답 1A). t1 줄 메뉴 「세션 끝내기」 → 확인 창 → 세션만 끝나고 「다시 열기」로 다시 뜸, coordinator 메뉴 「하나 더 띄우기」(맨 위 profile 이름을 단다) → coordinator pane 둘, 보고가 오면 host가 낸 30일 링크를 thread 화면을 연 채 설정 「이 aaa」 「낸 링크」에서 보고(앱 안 보기가 마지막 열람으로 남는다) 폐기하면 열려 있던 링크 줄도 「폐기됨」이 되고, 7일을 골라 새 링크(만료 − 발급 = 7일), 「나중」 파일은 폴더 밖 경로를 host가 거부한 말을 보이고 `memory/later.md`를 더하면 host `project show`의 `task_notes`에 들어가며 빼면 비운다.
- 결함 심기: 「CLI 0회」 검문 끄기는 첫 단계가, `do.message`가 받는 thread를 잃으면 자동화 ② 단계(전달 기록 없음)가, thread 관리 메뉴의 「끝내기」 빼기는 정리 단계가 잡았다. `do.message` 본문에서 규칙 글만 빼는 결함은 잡지 못한다: 실행 자료(`memo`)에도 같은 글이 들어가 화면에 남는다. 81은 「세션 끝내기」가 `pane_close`를 보내는 결함(「다시 열기」 없음), 「하나 더 띄우기」가 `new`를 뒤집는 결함(coordinator 하나), 링크 수명을 보내지 않는 결함(30일), 폐기 뒤 host가 reports 사건을 내지 않는 결함(열린 화면이 active로 남음)을 잡았다.

## 질문 dock 칩 정렬·답한 뒤 숨김(t-0687, plan §8.329)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 53-thread-questions --workers 1`: 실제 host의 질문 보고와 휴대폰 답변을 사용한다. Chromium 2800px·1280px와 WebKit 휴대폰에서 미응답 칩의 왼쪽 끝·열 오른쪽 끝이 입력칸과 1px 이내이고 입력칸 위에 있는지, 가로 넘침이 없는지 단언한다.
- 같은 세 크기의 답변 뒤 dock 칩·빈 dock 부재를 검사하고 두 상태의 스크린샷을 남긴다. 기존 대화 칩의 답함·보관된 질문 원문·다른 기기 카드·새 질문 회귀 검사는 유지한다. DOM·host 답 사건으로 끝내며 고정 sleep이나 시한 연장을 쓰지 않는다.

## SSH로 설치·등록(t-0578 웹 머신 추가 M3, plan §8.420)

- host 유닛 `cargo test -p aaa-host machine_setup`:
  - aaa 전송: 실험실 sshd로 보낸 파일이 원격 `mktemp -d` 폴더(0700)에 같은 SHA로 놓이고 `--version`이 돈다. SHA·판이 어긋나면 그 폴더를 지운다(보낸 파일과 같은 내용의 폴더만 세어 나란히 도는 시험과 섞이지 않는다).
  - 원격 install 대본: 실제 ssh로 보낸 shell 대본이 `install-probe`·`install-bootstrap`·`install --resume`에 미리 만든 답을 내고 받은 인자·stdin을 남긴다. 계획은 host가 보낼 choices로 실제 install planner를 돌린 것이다. 적용은 FIFO가 열릴 때까지 기다리고, 그사이 시험이 둘째 package의 grant로 host `install_enroll`을 부른다(실제 원격 worker 대신). 고정 sleep은 없다.
  - 원격 install이 이 SSH를 거부(exit 3)하면 까닭을 보이고 원격 임시 폴더·계획 파일을 남기지 않는다.
  - launcher(install I3): 보낸 choices가 `--service launchd`·`--start now`이고 원격 실행 파일 옆에 host 실행 파일 옆의 launcher package가 같은 내용으로 있다(대본이 계획 때 그 옆 파일을 남긴다). 다른 실행 파일 SHA에 묶인 package는 원격 계획 전에 멈추고 원격 임시 폴더를 지운다. 개발 판 host 옆에 없으면 보내지 않고, 원격이 `installation_launcher_asset_required`를 사람 일로 알리면 host가 준비할 것(package 이름·release 판 host)을 덧붙인다. release 번호 판인데 그 release에 launcher가 없으면(옛 release) manifest만 읽고 보내지 않으며 그 까닭을 덧붙인다. 대본 원격은 OS를 보지 않으므로 Linux CI에서도 감지 OS를 macOS로 둔다.
  - release 받기: 임시 HTTP 서버(받은 경로를 남김)로 정상·같은 SHA 캐시(asset 다시 안 받음)·SHA 다름(남기지 않음)·manifest 판 다름·그 판 release가 아닌 주소(manifest만 읽음)·없는 종류·옛 manifest의 launcher 없음. 감지 CPU를 다른 종류로 바꾼 작업은 release 실행 파일과 launcher(manifest `launchers`, 캐시는 실행 파일 옆 0600)를 받아 원격에 나란히 보내고 그 SHA를 원격 install executor로 준다.
  - 끝 확인·로그인: 끝 확인 스크립트를 이 머신에서 실제로 돌려 표지 사이 줄을 읽는다. 가짜 agent(duplex 연결)가 Spawn argv·cwd를 남기고, 그 스크립트 실제 출력을 pane 화면으로 넣으면 host가 작업에 줄을 남긴다. `codex login` pane, 끝나지 않은 작업·보기 기기·머신 없음·연결 안 됨 거부.
- 웹 유닛 `node --test web/test/machineSetupModel.test.mjs`(1.103 판정, macOS만 설치, 기본 설정, 설정 앞 검사, 단계).
- 웹 E2E `72-machine-add` 마지막 단계(harness가 `AAA_TEST_RELEASE_BASE`를 닫힌 loopback 포트로 줘 개발 빌드 host가 같은 판 launcher를 찾으러 실제 GitHub에 가지 않는다): 설치 설정 폼(기본 이름·작업 root, 잘못된 이름 알림) → 「계획 보기」가 실제 host 실행 파일을 보내고 실제 install이 비특권 실험실 sshd를 거부하는 까닭(시스템 SSH 서버)을 보인다. 원격 임시 폴더가 남지 않는다. 보기 기기에는 설정 폼이 없다.
- 실제 install SSH 입구(root 소유 시스템 sshd 계보)는 이 Mac 실험실에서 열 수 없다(시스템 설정을 바꾸지 않는다). 끝까지의 실제 경로는 CI 필수 단계 「SSH로 설치·등록 실제 root sshd·LaunchAgent」(macos-15)가 본다:
  - `scripts/ci-root-sshd.sh`(GitHub Actions가 아니면 멈춘다)가 실험실 host key·사용자 키·`sshd_config`(loopback 빈 포트, 비밀번호·PAM 끔, `AllowUsers` 자기 계정, `SetEnv HOME=<실험실>/h`. 실험실은 agent socket 길이 때문에 짧은 `/private/tmp/ar.XXXXX`)를 만들고 `sudo -n /usr/sbin/sshd`로 띄운다. 끝에 그 PID만 끝낸다.
  - ignored 시험 `cargo test -p aaa --test m13 ssh_install::ssh_install_through_the_system_sshd_starts_a_launch_agent_that_connects_and_checks -- --ignored --exact`(`AAA_ROOT_SSHD_LAB`): 시험 aaa 사본 옆에 I3 시험과 같은 방법으로 만든 launcher package를 두고 그 사본으로 host를 띄운다 → 「ci」 접속 → 지문(`ssh-keygen -lf`와 같음) 믿음 → 감지(원격 HOME = 실험실) → 계획(Review) → 적용 → 설치·등록됨 → agent 연결 → 끝 확인 줄(작업 폴더·PATH).
  - HOME 분리: 원격 셸·install·LaunchAgent(plist의 HOME)가 모두 실험실 HOME을 쓴다. 시험이 실험실 HOME 아래 plist·`.local/bin/aaa`를 보고, runner 실제 HOME의 같은 자리(plist·실행 파일·launcher·상태 폴더)가 전후 같은지 본다. gui domain label `dev.aaa.agent`는 계정 전역이라 적용 전 없어야 하고 시험이 끝에 bootout한다.
  - 스크립트는 시험 전에 그 sshd로 연 원격 명령의 조상(pid·ppid·사용자·image)을 적는다. install 검증 SSH 입구가 root `/usr/sbin/sshd`를 찾지 못하면 그 사슬로 원인을 본다.
  - 이 Mac에서는 같은 시험을 비특권 실험실 sshd(`SetEnv HOME` 포함)로 `env -u AAA_SESSION_ID`로 돌려 계획 단계의 검증 SSH 입구 거절까지(launcher 만들기·host·지문·감지 HOME·전송) 확인한다.

## 연결 안내의 0.5초 지연(t-0686, plan §8.424)

- `NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs <캡처 폴더> --spec 84-connection-banner --workers 1`: 실제 host·agent와 2,400행 Claude JSONL을 Chromium 1280px·WebKit 390px에서 연다.
- 시험 안 WebSocket의 open/message 전달을 gate로 보류한다. 실제 연결·기록 조회는 유지하고 Playwright 시계를 499/500ms로 전진시켜 지연 경계와 빠른 연결에서 순간 표시도 없음을 MutationObserver로 확인한다.
- pane 왕복의 지연 초기화, 실제 socket 닫힘 뒤 499ms 미표시·500ms 표시, 재시도 시 표시 유지, 회복 즉시 제거·옛 타이머 취소를 확인한다. 고정 sleep·재시도·시한 연장은 없다. 정본 전체 검증은 `nice -n 15 bash scripts/ci-local.sh`와 공개 검사다.


## Windows 최소 다운로드 bootstrap(t-0485)

- `cargo test -p aaa --test windows_bootstrap --locked`: Windows에서 PowerShell 5.1과 7을 모두 실제로 실행한다. 네트워크 요청만 외부 경계에서 대체하고 실제 PE/파일/hash/--version/native argv/종료 코드와 표준입력 상속을 검사한다. 빈 문자열·공백·따옴표·끝 역슬래시·한글·셸 메타문자를 보존하고, 같은 bytes의 mtime·다른 instance·충돌 파일을 보존한다. manifest 판/URL/hash·PE/CPU/실행 판·파일 부모·junction·NUL 거부 뒤 실행/배치가 없어야 한다. 실제 aaa dry-run은 state를 만들지 않고 --yes는 쓰기 권한이 아니다.
- 이 owner만 `cfg(windows)`다. PE/CreateProcess·PowerShell 5.1·NTFS junction이 대상 OS 계약이기 때문이다. macOS/Linux에서 통과를 주장하지 않는다. 공통 Rust install owner는 각 OS의 기존 Cargo 시험에 남는다. 실제 Windows 판정은 승인된 hp branch의 기존 windows-2025 CI에서 한 번 확인하고 branch를 지운다.
- `nice -n 15 python3 -m unittest scripts.test_release`: 같은 source의 bootstrap만 게시하며 manifest/체크섬이 실제 script bytes를 포함해야 한다. ci-local의 test 및 distribution workflow에도 묶었다. 검토하지 않은 script/누락 파일을 허용하는 변형이 이 owner를 실패시켜야 한다.
- CI는 임시 사용자 폴더만 쓴다. bootstrap 자체는 SCM/registry/PATH를 변경하지 않는다. 실제 SCM logon·Session 0·부팅·LSA/UAC·harness 인수는 [Windows 사용자 절차](windows-bootstrap.md)로 분리한다.


## install supervisor 잠금의 owner 종료(t-0485)

기존 `service_phase_keeps_file_resource_locks_and_removal_and_credentials_resume_preserve_context` owner는 Unix에서 supervisor File의 실제 복제 descriptor를 owner 종료 뒤까지 남긴다. fork와 같은 open file description이 남아도 원래 Job 종료 뒤 같은 ID의 credentials/removal 재개가 즉시 가능해야 하며, 옛 사본을 닫아도 새 Job 배타성은 유지해야 한다. Windows는 실제 share reservation 방식과 원래 전체 owner를 유지하며 Unix flock 복제 대조만 제외한다. 원래 File close만 쓰는 코드에서 installation_busy RED, install private::Lock guard drop의 명시 unlock 뒤 GREEN을 확인한다. 잠금 생존 중 거부·계정/설정/준비 검증·제거/보존/조회 단언과 기존 시한은 그대로다.

같은 Unix guard는 `preserving_a_launcher_binds_its_bytes_to_the_approval_before_any_transaction_write`의 짧은 발급 scope 뒤 실제 복제 descriptor를 남겨 issuer.issue 재획득 RED→GREEN을 확인한다. `consumption_survives_reopen_and_only_explicit_same_id_resume_is_allowed`는 Lease 복제 descriptor가 남아도 같은 ID만 재개되고 소비/서명/만료/읽기 보존 단언이 유지됨을 본다. 세 owner의 기존 단언·시한은 바꾸지 않는다. Windows는 복제 대조만 cfg 제외하고 원래 실제 share reservation·owner 전체를 유지한다.


### 배포 binary의 production 의존성 검사

ci-local build는 Windows에서 `cargo build --release --locked -p aaa --bin aaa --target x86_64-pc-windows-msvc`를 독립 호출한 뒤 기존 workspace/all-targets build를 이어 간다. 기존 windows-2025 CI도 이 단계를 실행한다. 배포와 같은 release profile·Windows target·production binary만 컴파일·링크하며 dev feature 합치기로 Win32 API 누락이 가려지는 것을 막는다. v0.1.3 release의 JobObjects dev-only 누락이 실제 실패 사례다. 필요 feature를 dev-only로 돌린 변형은 같은 production release 빌드에서 실패해야 한다. macOS의 `cargo xwin build`는 교차 컴파일·링크 증거이며 실제 Windows 실행 판정은 기존 CI가 맡는다. source 목록 검사·새 runner·시험 전용 export·시한 변경은 없다. 다른 OS의 기존 전체 build와 시험은 유지한다.

## Windows 한 줄 입구·부분 설치 재개(t-0485, plan §8.446)

- `nice -n 15 python3 -m unittest scripts.test_release`는 실제 producer 출력의 install.ps1 URL/파일/manifest/SHA256SUMS를 대조한다. Windows의 기존 `--test windows_bootstrap` owner는 PS5.1/7의 실제 IRM octet-stream→IEX·argv/exit·구판/부분 config 보존·내장 hash 거부를 검증한다.
- `nice -n 15 cargo test --locked -p aaa --test install recovery::`는 닫힌 실제 host 등록 창과 실제 PTY/서명/파일/journal에서 status ID/안내·명시적 재동의·같은 transaction을 확인한다. Windows 전용 같은 묶음은 실제 powershell.exe/pwsh.exe의 한 줄 입구가 외부 콘솔을 native 검토/승인/파일 적용까지 유지하는지 검사하며 서비스는 none이다. 새 production flag/export나 실제 운영 서비스 등록은 없다.
- `nice -n 15 cargo test --locked -p aaa --bin aaa install::engine::execution::tests::legacy_hook_alias_approval_keeps_its_signature_id_and_scope_through_status_and_resume -- --exact`는 서명 원문/ID/범위를 유지하는 옛 lexical hook 호환과 다른 target/SHA 거부를 검사한다. `install::engine::transaction::references::tests::`는 명시 단독 repair의 최초 원본 복원과 다른/공유 owner의 무쓰기 거부를 검사한다. 암호/issuer key가 포함된 실험실 전체를 로그에 출력하거나 library로 복사하지 않는다.
- 최초 RED와 수리 GREEN을 보존하고 canonical 비교/owner 제한을 각각 망가뜨린 변형 실패를 확인한다. 전체 ci-local·공개 검사와 Windows/Linux strict cross, dev 없는 Windows release build 뒤 착지 창을 기다린다. Windows 실제 판정은 CI이며 교차 빌드나 로컬 PowerShell로 대신하지 않는다.

## pane 책임 경로와 launcher 보존 재시작(t-0763, plan §8.449)

- `cargo test -p aaa --test restart --locked`: macOS에서 실제 launcher·host·agent·session-host·CLI를 쓴다. launchctl 호출만 격리된 자기 launcher에 신호를 보내는 script로 대체하며 운영 launchd 등록은 하지 않는다. fixture launcher는 기존 C 소스를 그대로 빌드하고, `AAA_RESTART_TEST_LAUNCHER`를 주면 그 기존 파일을 읽기 전용으로 사용한다. 등록된 launcher 바이너리를 바꾸지 않는다.
- 서로 다른 SHA인 agent 사본으로 바꾼 뒤 CLI 재시작으로 launcher·worker PID와 기존 pane이 유지되는지 확인한다. 기존 pane·새 pane에서 고유 코드를 넣은 C·Rust 실행 파일과 cargo run·build.rs의 실제 책임 PID를 확인하고 첫/재실행 시간을 보존한다. 시험 도구 `scripts/pane-responsibility-probe.py`의 출력은 작업 저장소 `target/restart-probes` 아래에 둔다(임시 OS 폴더의 Cargo target은 쓰지 않는다). 권한 미등록 CI에서는 속도 상한을 합격 조건으로 삼지 않는다.
- 잘못된 worker 생성 시각·승인과 다른 실행 파일은 launchctl 변경 호출 전에 거부한다. launcher를 직접 종료하는 별도 단계는 pane을 남기고 새 agent의 재연결에서 `detached`가 갱신되는지 확인한다. 시험은 준비 기록·host 사건·명령 완료 출력으로 끝낸다. 셸 화면은 끝 공백을 잘라 내므로 `$ ` 문자열을 준비 사건으로 기다리지 않는다.
- `cargo test -p aaa-agent --lib responsibility --locked`는 실제 경로 분류·조회 실패를 확인한다. 기존 직렬화/TypeScript 계약 검사는 선택 필드의 이전 판 호환을 유지한다.
- E2E89은 실제 host·agent·thread를 사용하고 관측값만 응답 경계에서 바꾼다. Chromium 1280/320px, WebKit 390px Reader의 경고·접힌 경로 상세·정리 창·가로 넘침·보기 권한·보류 무변경을 확인하고 캡처한다. 보고서 iframe이 필요 없는 이 시험은 「사람을 기다림」 fixture로 sandbox/ServiceWorker 주입을 분리한다. 실제 보류·저장·이어 하기는 기존 E2E85와 함께 검증한다. 목록과 터미널 표지도 직접 확인한다. t-0928부터 HTTP route 재처리 없이 context의 `Response.json()` 결과에서 책임 관측값만 바꾸며 실제 요청의 완료·취소 수명은 브라우저가 맡는다.
- 최초 회귀에서 준비 기록 상위 폴더 누락을 재현해 수정했다. 새 fixture launcher가 시작하지 않은 실행은 해당 PID의 syspolicyd YARA 오류·종료와 직전 DetachedSignatures EMFILE을 기록했다. 시스템 설정·시한·기대값을 바꾸지 않고 기존 등록 launcher를 사용하는 별도 경로에서 책임·첫 실행을 검증한다. 준비 기록 신원은 파일 경로 문자열 대신 커널 생성 시각·부모·UID·boot로 대조한다(원자 교체된 옛 실행 파일의 경로 조회를 신원으로 쓰지 않는다).
- 전체 검사에서 기존 task 경고 유일성 시험이 새 책임 안내의 `경고:` 접두어 때문에 실패했다. 책임 안내를 별도 `개발자 도구:` 줄로 표시해 기존 task 경고 계약·단언은 유지하며, 새 restart 회귀가 `pane list`의 기존 무인자 JSON 출력에 책임 관측값이 포함되는지 확인한다.
- 추가 CLI 시험의 최초 문구 단언은 pane 명령의 기존 JSON 전용 계약을 잘못 가정했다. 불필요한 pane 글 출력 분기를 없애고, `--json` 없는 실제 CLI 출력도 JSON 한 줄이며 책임 관측값을 포함하는지 검사한다. thread 글 출력의 별도 진단과 웹 표지는 유지한다.
- 전체 필수 검증은 `nice -n 15 bash scripts/ci-local.sh`·공개 검사·Linux 대상 clippy다. 착지 검증은 자기 worktree target만 쓰며 운영 host·agent 재시작은 배포 때 coordinator가 한다.
## Windows 한 줄 입구 첫 CI 수리(t-0485, plan §8.450)

- 기존 `--test install recovery::`는 실제 OS architecture 환경을 보존한 PS5.1/7 입구를 실행한다. 값을 AMD64로 꾸미지 않는다. 닫힌 실제 등록 창→파일/status ID/안내→같은 ID 새 nonce/job 하나를 모두 검사한 뒤 runtime·Host를 종료하고 lab을 제거한다. Windows OS32를 무시하거나 재시도하지 않는다.
- 기존 `legacy_hook_alias_approval_keeps_its_signature_id_and_scope_through_status_and_resume`는 Windows canonical verbatim 경로에서 일반 Win32 lexical 경로를 구성한다. 기존 lexical/canonical 차이·서명 원문/ID/scope/nonce·다른 target/hash 거부·status 무쓰기·재개 job 하나 단언을 유지한다. Windows/Unix 모두 실행하며 Windows만 scope 구성을 달리한다.
- `--test windows_bootstrap`의 staged executor exact argv 비교는 그대로 두고 실패 시 시험 argv의 actual_hex/expected_hex를 한 줄로 남긴다. 이 진단은 비밀 입력을 받지 않는 native fixture에만 적용한다. PS5.1에서 실패했다고 PS7까지 통과한 것으로 쓰지 않는다. 로컬 PowerShell의 조립 구문 대조나 Windows cross build를 실제 Windows 성공으로 보지 않는다.
- 최초 Windows RED는 main 4fe5937c의 run 39761138515/job 119765308652다. 새 exact main CI로 Windows GREEN 또는 staged argv의 실제 차이를 판정한다. 공식 전체 ci-local·공개 검사·양 OS cross clippy와 Windows release build 뒤 별도 착지 창을 기다린다. 시한/기대값/조합 수는 바꾸지 않는다.

## 완료 탭과 웹 기록 제거(t-0775, plan §8.453)

- host의 기존 `archive_search_pages_bound_bytes_keep_all_matches_and_restore_without_queue` owner를 확장한다. 4MiB 초과 원장을 실제 DB에 저장하고 전체 페이지·검색·저장소·완료 근거·복원 불변식을 유지하면서 모델·머신과 완료 기간의 시작 포함/끝 제외, 미지정과 전체, 필터 전 전체 선택지를 검사한다.
- E2E86은 웹 기록 대신 완료 탭을 검사한다. 실제 host 원장 130건의 페이지 분할·검색·저장소·모델·머신·기간·완료 근거 조합, 상세 메모·되살리기·Reader 쓰기 거부, 직접 체크 보관을 본다. Chromium 데스크톱/320px·WebKit 390px와 밝음/어두움을 캡처한다. E2E26 긴 목록의 기록 부분도 완료 79건의 끝 행·상세·다음 페이지로 바꾼다. E2E46은 삭제 직후 되살리기 인수를 유지하고 없어진 웹 기록 동선만 제거한다.
- 완료 사건은 API 성공 응답·aria-busy·DOM·파일 상태다. 고정 sleep·재시도·시한 변경은 쓰지 않는다. 전체 ci-local·공개 검사를 수행하고 착지 창 전에는 병합·push하지 않는다.

- E2E86은 실제 복원 쓰기의 응답만 gate로 보류한 뒤 다른 완료 검색을 열고 응답을 푼다. 이전 검색을 다시 읽던 코드에서는 결과 0건으로 실패했고 최신 load를 참조하도록 고친 뒤 새 검색 1건을 유지한다. 최초 시험 자료의 저장소 불일치를 바로잡은 뒤 RED→GREEN을 확인했으며 시한·제품 기대값은 완화하지 않았다. E2E47은 상세의 메모와 실제 r-ID 보고서 연결도 검사한다.

## 계정 gate 수신 진단(t-0777)

`t-0775`의 기존 `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials` 실패는
`expected value`, line 1, column 1이다. 같은 잠금 파일의 serde_json에서 빈 입력은
`EOF while parsing a value`, line 1, column 0이고, HTTP 요청 등 JSON이 아닌 첫 바이트는 원본 오류와 같다.
원본 로그에는 받은 문자열·peer가 없어 빈 연결이나 송신 프로세스 종료로 확정할 수 없다.

- Gate는 EOF를 별도로 단언하고 JSON 오류에는 예상 phase, listener·peer, 받은 바이트 수와 첫 8바이트의 hex를 남긴다. 입력 본문 전체는 로그에 쓰지 않는다.
- `POLICY_HARNESS`는 실험실 `proof/gate-trace.log`에 JSONL로 PID·thread·phase·클라이언트 주소와 연결/전송 완료를 기록한다. 토큰과 인증 홈은 이 진단에 넣지 않는다. 실패하면 기존 Lab 정리가 실험실을 보존하므로 peer를 이 기록과 대조한다. CI 실패 요약은 `*.log`·`*.json`만 모으므로 확장자를 `.log`로 둔다.
- 기존 phase·계정·토큰·pane 수·종료 단언과 30초 상한을 유지한다. EOF나 잘못된 JSON을 건너뛰거나 연결을 다시 시도하지 않는다. 진단 추가를 원인 수리 완료로 보지 않는다.

### 정해진 20회의 동시 부하 재현 조사

- 전체 Rust에 포함된 대상 owner 1회와 별도 프로세스 19회를 합쳐 총 20회로 고정했다. 별도 실행은 최대 4개를 동시에 돌리고, 정본 `scripts/rust_tests.py`의 전체 workspace·doctest 28묶음(worker 6·libtest thread 2)과 겹친다. 먼저 binary를 모두 준비하고 실행 중 같은 aaa 실행 파일을 재빌드하지 않는다. 실패를 재실행하거나 횟수를 늘리지 않는다.
- macOS·논리 CPU 10개에서 대상 20/20과 전체 Rust 28/28묶음이 통과해 원본은 재현되지 않았다. 별도 19회의 시작/끝 시각 모두 전체 Rust 실행 구간 안에 있었다. 전체 Rust는 219.15초, 별도 대상 실행은 각 33.03~39.75초였고 시작/끝의 1분 부하는 15.58~43.19였다. PID·시각·종료 코드·로그를 각각 보존했다. 이 결과로 원인을 고쳤다고 판단하지 않는다.
- 이후 CI 수집 대상에 맞게 trace 확장자만 `.jsonl`에서 `.log`로 정리했다. 기록 형식·송신 순서·기존 단언·시한은 그대로다. 제품 코드는 바꾸지 않고 진단 변경만 준비하며 다음 CI·ci-local의 재발 자료로 송신자를 확인한다.
- 실제 `POLICY_HARNESS`를 TCP gate와 시험용 hook/거부 CLI에 연결해 두 phase의 연결/전송 기록 4개, PID·peer 일치, 인증 값 없음과 기존 CI `find` 수집 조건에 포함됨을 확인했다. 대상 owner의 추가 실행은 하지 않았다.

## Windows bootstrap profile 표기 수리(t-0485, plan §8.454)

- 기존 `cargo test --locked -p aaa --test windows_bootstrap` owner의 staged argv exact 비교를 유지한다. profile directory 생성 뒤 입력을 OS GetFullPath로 정규화해 PS5.1의8.3별칭 확장과 fixture 기대target 표기를 맞춘다. actual을 바탕으로 expected를 만들지 않는다.
- RED는 main36f3e935의 Windows job119808659146에 남긴 actual_hex/expected_hex다. 세 인자가 동일하고 profile ancestor만 긴 이름/별칭인 것을 전체 로그에서 확인했다. 원래 argv/exit/hash/구판·부분 config 보존·거부 조건·PS5.1/7·실제 IRM/IEX 범위와 시한은 유지한다. production 변경은 없다.
- macOS parser/release producer·전체 ci-local·공개 검사·양 OS strict cross와 Windows release 조건 build는 호환 검증이다. 같은 owner의 실제 Windows GREEN은 후보 착지 뒤 main CI에서만 판정한다. CI rerun/단언 완화로 넘기지 않는다.


## 저장소 workspace·기존 결과 지표(t-0784, plan §8.455)

- `nice -n 15 cargo test --locked -p aaa-agent --lib workspace_metrics`: 기존 형식의 fixture만 읽는다. 전체/단계 경과·개수·E2E 파일/작업 분리, 묶음을 개별 테스트 시간으로 쓰지 않음, 반복 E2E의 중간값 제외, 손상·누락 자료의 미수집을 확인한다. rc·엔진은 기존 개별 exec_time·시나리오 단계 합계와 이전 실행 제외를 검사한다. rc 시간표를 엔진 입력으로 오인하지 않고, 읽기 예산을 넘겨도 이미 확인한 전체 경과를 보존하는지 확인한다. 수집 뒤 원본 로그를 지워도 같은 입력 판은 저장한 요약을 재사용한다. 실제 Git 커밋 객체로 최초/증분·rename·삭제·dirty 파일 제외·혼합 cfg(test) 합계와 host가 가진 커밋의 재집계 생략을 검사한다. 실패한 동일 커밋을 다시 스캔하지 않고 다음 커밋에서 회복하는지도 검사한다. Unix에서는 fork가 물려받는 것과 같은 열린 파일 참조를 남겨도 소유자 종료 때 수집 잠금이 풀리는지 검사한다.
- `nice -n 15 cargo test --locked -p aaa-host --lib workspace_metrics`: SQLite 재시작·중복·명시적 workspace 묶음의 이력 이전, 프로젝트 이름 변경·삭제, API Reader 조회·기간/머신/profile·빈 결과·미수집을 확인한다. 날짜별 p50은 머신·profile 경계를 지키고 실패·빈 날짜를 넣지 않는다. 자료가 없는 등록 경로로도 조회할 수 있어야 한다. 조회가 원본을 읽거나 agent에 요청하면 실패하는 경계를 유지한다.
- `NODE_PATH=$(npm root -g) nice -n 15 node web/e2e/web.e2e.mjs <캡처 폴더> --spec 90-workspace-metrics,26-activity-bar --workers 1`: 실제 격리 host·agent·보고 감시·전송·원장·웹을 사용한다. fixture는 기존 runner 종료 산출물만 준비한다. 수집 완료 사건으로 열린 화면이 갱신되는지, 전체·개수·TOP 5·dirty 코드 제외, URL 복원·필터·키보드 상세·목록 접기·로그 삭제 뒤 조회를 확인한다. Chromium과 WebKit, Reader, 390/320px, 밝음/어두움에서 drawer·가로 넘침·오류 때 마지막 성공 값을 검사하고 캡처한다. 데스크톱은 1280px이다. 실제 iPhone 검증은 아니다.
- 완료 사건은 보고 hash·원장 저장 뒤 WebSocket 사건·DOM·응답이다. 통계를 얻으려고 추가 검증을 실행하거나 검증의 옵션·병렬 수·시한·성공 조건을 바꾸지 않는다. 필수 전체 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사·의존 경계 시험이다.


## 질문 답과 닫힌 질문 정리의 post 종료 경합(t-0801)

- `nice -n 15 cargo test --locked -p aaa-host --lib question_answer_and_reconcile_handle_each_post_once`: 실제 Host·SQLite·질문 답 보존을 사용한다. 질문에 연결된 post 두 건을 만들고 답 전송 뒤의 연결 post 처리와 닫힌 질문 정리가 모두 열린 목록을 읽은 뒤 종료하도록 시험 전용 barrier로 고정한다. 두 경로가 끝나는 join 사건으로 완료하며 post별 처리 알림 한 건·합계 두 건·답한 질문 하나와 반복 호출의 멱등성을 검사한다.
- 종료 알림 저장 실패 시 post 종료까지 롤백되는지, 읽음 표시가 완료 상태를 되살리지 않는지, 다른 프로젝트·이미 끝난 항목을 바꾸지 않는지 DB 경계 시험으로 확인한다. coordinator 구조화 질문의 답 트랜잭션 뒤 파일 이동·웹 갱신도 기존 질문 시험으로 확인한다.
- 기존 E2E53의 `request handled` 기대값 두 건을 유지한다. 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사·Linux 대상 clippy를 수행한다. 고정 sleep·재시도·시한 연장·단언 완화 없이 확인하고 착지 창을 기다린다.

## Windows 한 줄 입구의 probe 콘솔과 단계 진단(t-0485, plan §8.457)

- 기존 `recovery::one_line_entry_keeps_the_external_console_through_review_and_signed_file_installation`은 PS5.1/7의 실제 IRM/IEX와 native 질문·승인·파일/status를 유지한다. 실패할 때 원래 PTY panic 뒤 `one-line owner stalled` 한 줄에 shell·고정 stage·HTTP 단계·파일 존재 여부를 남기고 같은 panic을 재개한다. 성공은 실제 entry_returned를 기록한다. HTTP 단계는0=시작 전,1=accept 대기,2=접속,3=header 완료,4=본문 전송 완료다.
- `powershell_started`/`irm_entry`/`manifest_request`/`manifest_written`/`payload_request`/`payload_written`/`entry_returned`는 시험 transport의 고정 token이다. 마지막 transport token과 native 진행 단계를 혼동하지 않는다. 생산 승인/실행을 대신하지 않으며 trace 파일은 격리 lab에만 둔다. 별도 관찰용 시험·가짜 승인·시한 증가·단언 완화는 없다. macOS에서는 Windows owner가 cfg로 제외되므로 cross와 실제 Windows 결과를 구분한다.
- 진단 CI39794689113은 payload_written 뒤0/16 정지,39796210580은 다른 이미지와 실행 줄 trace 조건에서25/25 통과였다. 그 성공만으로 결론내리지 않고39798896583에서 같은 실패 이미지/runner로 원본 대조 RED와 Capture에만 CreateNoWindow를 주는 GREEN을 확인했다. 원본은 yes 이후 두 aaa 자식이 살아 있었고 lab cleanup OS32까지 이어졌다. 수리는 원래25개·PS5.1/7·모든 승인/상태/보존 단언을 통과했다. 이 대조가 console 분리 수리를 제거했을 때의 회귀 증거다.
- 일회성 workflow·임시 OS process 관측·Set-PSDebug는 최종 수리 후보에서 제외한다. 실제 install의 외부 console은 그대로이고 stdout/stderr를 캡처하는 버전 probe만 console 없이 실행한다. 전체 ci-local·공개 검사·Windows/Linux strict cross와 dev 없는 Windows release 조건 build를 확인한다. 이 진단 CI는 게시 전 main 네 job 판정을 대신하지 않는다.

## Windows SSH 설치와 서비스 암호의 대화 격리(t-0815)

- `cargo test --locked -p aaa-host --lib machine_setup::`는 Windows choices/드라이브 경로/고정 PowerShell 명령, 실제 실험실 SSH의 보호 frame·EOF, 원격 echo/오류 가림, Controller 웹 전용·Local/세션/Reader/worker/evaluator/detached 거부와 host 조회/DB/WAL·시험 영속 자료의 원문 부재를 검사한다. 가짜 암호는 실행마다 메모리에서 생성하고 assertion/fixture 파일/명령 인자/환경/출력에 넣지 않는다. 소유한 시험 폴더만 읽으며 실제 사용자의 대화/자격 증명을 읽지 않는다.
- `cargo test --locked -p aaa --test install session_secret_input_is_refused_before_stdin_or_any_persistent_write`는 자식에 `AAA_SESSION_ID` 또는 `AAA_HOOK_SOCK`를 주고 stdin을 열린 채 둔다. 공개 install은 EOF 전에 needs_user/3과 정확한 거부 code가 와야 하고 상태 폴더를 만들지 않아야 한다. 명시적 전역 --state-dir를 준 숨은 install-worker 직접 호출도 같은 세션에서 입력을 읽기 전에 3으로 끝나야 한다. 보호 frame parser와 현지 console reader에도 같은 문을 둔다.
- E2E72의 `machine-password.mjs`는 Windows 계획 요약만 fixture로 주고 입력→POST→실제 host의 없는 job 거부를 실행한다. Chromium/WebKit 각각 데스크톱/휴대폰에서 전용 type=password·autocomplete off·대화 붙여 넣기 금지 안내, 취소 뒤 송신 없음, 한 번 송신/실패 뒤 비우기, pagehide 뒤 비우기를 확인한다. 원문은 전용 apply 필드 외의 요청/응답·console·화면·다른 입력·localStorage/sessionStorage·URL/history에 없어야 한다. 실제 host의 시험 root 전체(DB/WAL·프로젝트/질문/inbox/task/report/library·실험실 harness 기록)를 읽어 원문 부재를 확인한다. 운영 ~/.claude·~/.codex 기록에 접근하지 않는다. 가짜 값은 실패 메시지·스크린샷/내보낸 자료에 넣지 않는다.
- `scripts/ci-windows-sshd.ps1`은 Windows GitHub Actions에서만 실행한다. 시스템 OpenSSH가 없으면 실패하며 기존 runner 계정/sshd 설정을 바꾸지 않는다. 일회 관리자 계정·임시 SYSTEM 서비스·loopback 포트·실험실 키를 만들고, 가짜 서비스 암호는 부모 메모리에서 시험 자식 stdin으로만 준다. `machine_setup::tests_windows::system_sshd_installs_a_current_user_scm_agent_and_registers_it`가 실제 파일 전송/SHA/판→검증 SSH probe→서명→bootstrap→암호 없는 needs_user→틀린 가짜 암호 거부→같은 ID의 보호 입력 재개→SCM agent의 TLS 등록을 확인한다. 종료 시 이 시험의 계정과 서비스만 정리한다. 원문이 시험 stdout/stderr에 있으면 CI에도 출력하지 않고 실패한다. 새 서비스·실제 사용자 logon/Session0의 CI 결과와 사용자 Windows PC의 재부팅/로그오프/실제 harness 인증 인수는 구분한다.
- Windows의 별도 SSH 설정 파일 `-f`는 [Microsoft OpenSSH 구성 문서](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-server-configuration)를 따른다. 서비스 등록은 일회 runner에서만 수행한다. 정본 로컬 검증은 `nice -n 15 bash scripts/ci-local.sh --tee`와 공개 검사, Windows cfg는 `cargo xwin clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings`다. 교차 컴파일 성공을 실제 Windows owner 성공으로 대신하지 않는다.
- SYSTEM sshd의 host key DACL(t-0827): `ssh-keygen`은 생성한 private key에 runner의 명시적 ACE를 둔다. 폴더의 상속 권한과 key 소유자만 바꾸면 이 ACE가 남아, runner의 `sshd -t`는 통과해도 SYSTEM 서비스는 key를 거부한다. [OpenSSH의 파일 권한 검사](https://github.com/PowerShell/openssh-portable/blob/59aba65cf2e2f423c09d12ad825c3b32a11f408f/contrib/win32/win32compat/w32-sshfileperm.c)는 현재 사용자를 기준으로 key의 owner와 ACE를 검사한다. 일회 host key의 DACL을 reset한 뒤 상속을 끄고 SYSTEM/Administrators만 명시적으로 허용하며 소유자는 SYSTEM으로 둔다. user key와 기존 sshd는 바꾸지 않는다.
- 시작 실패는 원래 오류로 끝내면서 `sc queryex/qc`, CIM 종료 코드, 두 파일의 ACL, 시작 이후의 SCM/Application/OpenSSH 사건과 서비스의 `-E` 로그를 출력한다. 별도 `sshd -ddd -e -f`는 runner의 단독 실행과 SYSTEM 서비스를 구별하는 자료이며 listener/EOF 사건 또는 진단 상한에서 끝낸다. 생성한 암호는 모든 진단 출력에서도 가린다. 암호·환경·private key 본문은 수집하지 않는다. 서비스 암호의 stdin 경로와 실제 owner의 모든 단언·시한은 유지한다.
- 시작 수리 뒤 Windows 실제 owner는 감지에서 멈췄다. 같은 SYSTEM sshd의 실제 명령 진단에서 cmd의 `ver`와 PowerShell은 0, 제품의 POSIX 감지 명령은 경로 오류와 255였다. 인증된 원격 명령도 255를 낼 수 있어 [SSH 종료 코드](https://man.openbsd.org/ssh.1#EXIT_STATUS)만으로 연결 실패와 구별할 수 없다. 믿는 키로 고정 `echo AAA-SSH-1` 접속을 확인한 뒤 POSIX→PowerShell 감지를 실행한다. 키 변경·인증 거부 판정과 BatchMode는 유지한다.
- `machine_setup::tests::a_trusted_connection_reaches_powershell_after_the_posix_command_exits_255`는 실제 실험실 SSH의 인증·키 확인을 유지하고 강제 명령으로 Windows cmd의 감지 계약만 모의한다. 키 승인 전에는 명령이 실행되지 않고 승인 뒤 echo→POSIX 255→PowerShell 순서로 Windows Ready에 도달해야 한다. 기존 코드는 같은 keeper에서 Failed로 끝났고 수리 뒤 통과했다. Windows 실제 owner를 이 Unix 모의로 대신하지 않는다.
- Windows SSH stdin(t-0827): PS 5.1의 .NET Framework `ConsoleStream`은 OpenSSH의 overlapped pipe에서 멈춘다. [PowerShell의 분석](https://github.com/PowerShell/PowerShell/issues/14478#issuecomment-1064764004)과 [SSHProcessMediator](https://github.com/PowerShell/PowerShell/blob/99a49e0eab9b137449b690ab4607d7932c6f9956/src/System.Management.Automation/engine/remoting/server/OutOfProcServerMediator.cs#L526)처럼 `GetStdHandle`을 비소유 `SafeFileHandle`·`FileStream`으로 열어 binary 업로드와 자식 install의 stdin에 복사한다. EOF·보호 frame·시한은 유지하며 입력은 C# helper 소스에 넣지 않는다. 실제 Windows에서 일반 자식·SSH PS 5.1/7의 256 KiB 해시를 대조하고, SYSTEM sshd에 제품 명령으로 PE 업로드/SHA/판과 자식으로의 1 MiB 원시 입력을 확인한다.
- SSH로 시작한 Windows host의 자식 `ssh.exe`는 새 stdio 핸들에 OpenSSH 조상의 fd 표를 적용하면 안 된다. [Win32-OpenSSH의 fd 초기화](https://github.com/PowerShell/openssh-portable/blob/274893f8d1fe86c4d128b514c3af4bc10cdc0c99/contrib/win32/win32compat/w32fd.c#L132)가 같은 조상 상속 문제를 지적한다. 자식 환경에서 해당 내부 변수 하나를 제거하며 사용자·시스템 환경은 바꾸지 않는다. 실기 대조에서는 원본이 네이티브 입력조차 멈췄고 제거 뒤 같은 시한으로 완료했다. 이 실험 환경 문제와 PS 5.1 ConsoleStream 결함을 구분한다.
- 지속 사용하는 Windows 실기에는 CI 전용 스크립트를 그대로 실행하지 않는다. 먼저 전용 경로의 정리 도구와 서비스·예약 작업·HKCU/HKLM Run·사용자/시스템 PATH·방화벽·사용자 기준 목록을 만들고 LOCK으로 시험을 직렬화한다. 기존 sshd/22번/설정·사용자 aaa는 보존한다. 만든 자원을 정확한 이름·직접 확인한 PID로 지운 뒤 기준을 대조하고, 차이가 없으면 실험 폴더도 지운다. 실제 서비스 계정 암호는 쓰지 않는다.

- E2E72 첫 실행과 진단 실행은 WebKit의 service worker가 활성화된 context에서 list fixture를 한 번도 받지 않아(list=0), 실제 실패 작업만 남은 화면의 설치 단추를 기다렸다. 이 화면 fixture context만 기존 다른 UI fixture처럼 serviceWorkers=block으로 고친다. 실제 UI/host 요청·암호 지우기·보관 부재 단언과 30초 상한은 유지하며 PWA/service worker 자체의 기존 시험은 바꾸지 않는다.

## Windows 한 줄 owner의 manifest fixture pipeline 제거(t-0485, plan §8.462)

- Windows 실패 원문(run39808392236/job119927674887)은60.005초에 질문0/16, child 생존, reader 미완료, `manifest_request`/HTTP4, executor/config/active=false다. 시험용 `ConvertTo-Json | Set-Content`가 끝나지 않은 범위까지 확인했으며 cmdlet 내부 원인은 단정하지 않는다. 앞선 다른 main의 owner 성공이나 native probe 수리를 이번 재발의 성공 근거로 쓰지 않는다.
- owner는 격리 lab에 manifest를 미리 쓰고 mock `Invoke-WebRequest`에서 그대로 복사한다. 실제 IRM/IEX 입구와 제품 manifest decoder·버전/URL/SHA·PE·native 검토/서명/파일/status 경계는 유지한다. 기존 PS5.1/7 루프·16질문·60초 상한과 모든 사후 단언을 유지하고 HTTP fixture 안의 JSON serializer pipeline만 제거한다.
- `nice -n 15 cargo test --locked -p aaa --test install`과 전체 `nice -n 15 bash scripts/ci-local.sh`·공개 검사·Windows strict cross clippy/build로 검증한다. 이 owner는 cfg(windows)이므로 macOS 통과나 cross만으로 Windows GREEN이라고 하지 않는다. 새 main의 Windows 실제 owner 결과가 남아 있다. 실제 CI 재실행·시한 증가·기대값 변경은 없다.

## Windows 서비스 thread 준비와 pane 입출력(t-0902, plan §8.468)

- 원본 서비스의 빈 `thread-preparations`와 Administrators 소유권을 읽기만 확인했다. 별도 시험 디렉터리에서 `CreateFileW`는 성공하지만 `ReOpenFile`은 같은 접근 권한과 `FILE_FLAG_BACKUP_SEMANTICS`에도 Win32 5를 반환했다. 변경 전 `windows_acl_tests::state_and_tls_are_private_on_creation_and_legacy_reload`도 같은 실제 Windows 오류로 실패했다. 기본 token owner와 현재 SID가 다를 때만 최초 handle에 `WRITE_OWNER`를 요청하고 그 handle에서 기존 소유자·일반 객체를 검증한 뒤 ACL과 소유권을 좁힌다. 경로를 다시 열거나 권한을 넓히거나 OS privilege를 켜지 않는다.
- `windows_acl_tests::inherited_thread_journal_directory_is_normalized_without_changing_parent`는 private state 아래에 일반 생성한 journal 폴더의 owner 이행·두 번 보호·private 기록 읽기와 부모 ACL 보존을 검사한다. 기존 TLS 신원 보존·공개 ACL 축소·hard link·다른 종류 거부 시험도 유지한다. 실제 Windows에서는 `cargo test --locked -p aaa-common --lib windows_acl_tests:: -- --nocapture`로 실행한다.
- `scripts/test-windows-service-pane.ps1`은 실행자가 기준 목록을 저장한 `C:\aaa-test\t-NNNN` 안에서만 쓴다. 전용 `aaa-test-…` SCM 가상 계정을 [CreateServiceW의 계약](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-createservicew)에 따라 NULL password로 만들고, 해당 폴더에만 접근을 허용한 뒤 원래 ACL로 돌린다. 실제 host·agent·thread 준비·session-host·ConPTY와 가짜 harness의 ready→입력→pong을 확인하고 네 프로세스가 모두 Session 0임을 검사한다. 실제 암호·사용자 서비스·설치·sshd·전역 PATH·방화벽은 바꾸지 않는다. 사용자 Codex 인증이나 실제 Codex TUI의 인수를 대신하지 않는다. 자기 서비스·프로세스·private 시험 디렉터리는 정리하고 결과·로그만 남긴다. 최종 실행자는 전역 기준 목록 대조 후 자기 시험 폴더도 지운다.
- PowerShell 5.1용 원본은 UTF-8 BOM으로 저장하고 자식 출력도 UTF-8로 읽는다. `-ExecutionPolicy Bypass`는 시험 자식 process 인자에만 주며 저장된 정책을 바꾸지 않는다. 준비는 로그 줄·CLI 종료·제품 `pane wait-output`·SCM 종료 사건으로 확인한다. 고정 sleep·실패 재시도·시한 증가·단언 완화는 없다. 오류에는 thread 준비·자리·journal 권한·brief·pane 명세·PTY·자식 실행 단계와 해당 경로를 붙이며 인자 전체·환경값은 덧붙이지 않는다.
- [WezTerm portable-pty ConPTY](https://github.com/wezterm/wezterm/blob/main/pty/src/win/conpty.rs)([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)), [Windows Terminal의 ConPTY 구현](https://github.com/microsoft/terminal/blob/main/src/winconpty/winconpty.cpp)([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE)), [OpenSSH for Windows의 PTY 실행 분리](https://github.com/PowerShell/openssh-portable/blob/latestw_all/contrib/win32/win32compat/w32-doexec.c)([BSD 계열 고지](https://github.com/PowerShell/openssh-portable/blob/latestw_all/LICENCE))를 먼저 조사했다. PTY·자식 실행·입출력 경계를 나누는 방식을 참고하며 코드나 의존성은 복사하지 않는다. ConPTY를 원인으로 가정해 우회하지 않는다. 정본은 전체 ci-local·공개 검사와 Windows strict cross clippy·실제 SCM owner다.


## Harness 판 조회와 계정 pane 실패(t-0910, §8.471)

- 공통 `harness_version`은 임의 Codex·Claude 판, CRLF, 조회 종료 7, 실행 파일 없음에도 판정 없이 진단 문자열을 반환하고 인증을 가리는지 본다.
- m13 계정 owner는 판 조회가 개발판·종료 7을 반환해도 실제 계정 pane·동시 갱신·격리·재개가 진행되는지 확인한다. 판 조회 실패에도 실제 session-host를 실행하며, 격리 인증 홈 symlink 거부가 thread 실패 기록까지 PID·종료·원인 사슬과 함께 전해져야 한다. 이미지 owner의 과거 판 거부는 임의 판 생성 성공으로 교체하며, 사용량 RPC owner도 임의 판·종료 7 뒤 실제 조회를 확인한다.
- Windows 서비스 시험의 선택 `-Codex C:\aaa-test\t-NNNN\codex.exe`는 전용 폴더의 실제 Codex 사본과 가짜 중앙 인증으로 별도 thread pane을 열어 SCM Session 0의 실제 Codex PID·실행 파일·running pane과 판 시작 기록을 확인한다. 화면 출력은 별도로 보존하며 TUI 상호작용 성공으로 해석하지 않는다. 운영 인증·설정·서비스는 읽기만 한다. SCM Session 0·가상 계정·입력/pong·기준 목록 복원 검사는 유지한다.
- 참고는 WezTerm portable-pty의 ConPTY 오류 전파(MIT), Nushell `run_external`의 외부 명령 오류 사슬(MIT)이다. 외부 코드·의존성은 추가하지 않는다. 로컬 전체 ci-local·Linux cross clippy·공개 검사와 Windows 실기를 수행한다. 고정 sleep·시한 연장·재시도는 쓰지 않는다.

## Windows Codex 빈 화면과 초기 DSR 교착(t-0910, §8.472)

- 수정 전 Windows 시험 서비스의 raw PTY 최초 출력은 `[27,91,54,110]`(DSR) 한 번이고 질의 응답은 없었다. 같은 인증 가림 규칙에서 `Stream.push(ESC[6n, false)`는 마지막 n을 보류한다. 가림과 ConPTY가 서로 다음 출력을 기다리는 경계를 재현하며 운영 pane·서비스에는 입력하지 않는다.
- `aaa-session output::tests`는 첫 DSR의 0~4바이트 분할, 인증 접두부 7바이트를 붙잡은 상태의 즉시 DSR·DA1·DA2 응답, 질의 앞뒤 인증 조각 전체 가림·공개 replay 비노출, resize 후 커서, 색 지정/되돌리기와 끝나지 않은 escape의 EOF 가림을 검사한다. `aaa-vt query_state_keeps_no_raw_scrollback_after_resize`는 resize 뒤에도 원문 scrollback이 생기지 않는지 확인한다.
- m13 계정 owner의 실제 로컬/TLS 원격 PTY는 raw mode에서 첫 DSR만 쓰고 응답을 받기 전까지 출력·인증·갱신을 시작하지 않는다. 기존 계정 동시 갱신·부분 회전·가림·재개 단언과 사건 시한은 유지한다.
- 참고는 [Microsoft Terminal ConPTY 연결](https://github.com/microsoft/terminal/blob/main/src/cascadia/TerminalConnection/ConptyConnection.cpp)(MIT), [WezTerm portable-pty](https://github.com/wezterm/wezterm/blob/main/pty/src/win/pseudocon.rs)(MIT), [xterm InputHandler](https://github.com/xtermjs/xterm.js/blob/master/src/common/InputHandler.ts)(MIT)다. 질의 응답과 출력 처리 경계를 참고하며 외부 코드는 복사하지 않는다. Windows 재현용 raw 계측은 제품 커밋에서 제거한다.
- 정본은 전체 ci-local·공개 검사·Linux cross strict clippy와 Windows 시험 서비스의 실제 Codex 화면·brief 전달이다. 운영 인증은 사용하지 않고 가짜 API 인증·loopback 주소로 시험한다. 결과·로그 보존 뒤 전용 서비스·확인한 PID·시험 폴더를 지우고 기준 목록을 대조한다.

## 전체 검증의 셸·snapshot·목록 준비 경계(t-0915)

- E2E93 실패 원본 pane spec의 argv는 비어 있었고 머신 기본 login shell을 골랐다. PTY 입력 ACK 뒤에도 harness 준비 표지가 없었다. 일반 셸에서 연 harness를 검사하는 이 시험은 공통 t1 실험실과 같은 명시적 `/bin/sh`를 써 머신의 개인 셸 초기화에 기대지 않는다. 16개 조합·단계 이름·20초/120초 상한은 유지한다.
- E2E97 원본은 작은 snapshot 뒤 약 499 KB의 live 출력을 받았고, 앞 기록 요청은 0줄·more=false였다. 출력 중 자동 재접속이 열려 중간 snapshot을 받은 경합이다. 다음 WS의 server 연결만 보류하고 실제 PTY `COMPRESS_READY>` 뒤 놓아 전체 출력을 본 snapshot으로 Worker·ACK·두 번 prepend를 검사한다. 회선 속도·compression·위치 기대값은 바꾸지 않는다.
- E2E26 원본은 WebKit desktop의 목록 행 평가에서 `closest(selector)`가 null이었다. DOM 교체 전에 고른 행과 나중 부모를 섞지 않고 같은 DOM 평가에서 현재 컨테이너·마지막 행의 경계·hit test가 맞는 사건을 기다린다. 기존 실제 overflow·휠·스크롤 끝·머리/바닥·행 선택 단언을 유지한다.
- E2E66은 원본 load average 50~60에서 네 기기의 계측만 약 136초여서 단계 120초를 넘었다. 코드를 바꾸지 않고 부하가 내려간 뒤 독립 실행하면 같은 호출 수·nativeRows·CSS 쓰기·cache hit 수로 37.5초에 끝났다(네 기기 계측 8.0~10.9초, 원본 28.8~38.9초). 원본 실패를 남기고 부하 근거와 독립 통과를 함께 기록한다. 시한·기대값·조합은 유지한다.

## 가짜 Claude의 화살표와 신뢰 질문 중복(t-0915 전체 검증)

- 전체 검증 원본에서 신뢰 질문에 Yes를 보낸 뒤 438ms 안에 같은 trust 질문이 다시 생겼다. 원본 다섯 질문 기대값에 trust/Gone 하나가 추가되어 실패했다. 보관함 코드는 이 신뢰/brief 단계에서 호출되지 않았다.
- 실제 PTY에 `Esc [ B Enter`를 한 번에 넣고 선택을 다시 그리는 경계에서 멈추면 기존 fixture가 사이에 Working을 쓴다(RED). 단일 Esc를 바로 지우는 처리가 화살표에도 적용되어 확인창을 잠깐 닫고 다시 열었다.
- 메뉴만 최대 3바이트를 한 번에 읽어 완전한 화살표는 선택만 옮긴다. 나눠 온 Esc suffix와 단독 Esc의 기존 처리는 유지한다. 새 PTY keeper는 선택을 옮기기까지 Working이 한 번도 없고 질문이 유지되는지 검사한다. 기존 Esc keeper·원래 질문 5개·interrupt·질문/메시지 인수 단언은 유지한다. 고정 sleep·시한·기대값을 바꾸지 않는다.

## 보고서 보관함과 완료 보고 기본 표시(t-0915, plan §8.476)

- `report_ids::tests`는 한 판만 보관, 재발행 뒤 원문 고정, 자동 정리 뒤 판 열기, 재시작·명시 삭제, 일괄 요청의 사전 검증·Reader 거부, 보관 변경 뒤 정리 미리보기 충돌을 검사한다. 실제 폴더의 미등재 파일·manifest 없는 폴더와 수집 중 임시 파일을 세고 symlink를 따라가지 않는 경계도 확인한다.
- `library_links::tests::archive_alone_can_exceed_the_limit_without_evicting_new_reports`는 1 MiB 상한에 1.1 MB 보관 판과 0.2 MB 일반 사본을 만들어 보관함이 상한을 소비하지 않는지 확인한다. 최신 별칭·최신 r 판을 용량 경계에서 함께 정리하고 옛 보관 판은 유지하는 회귀도 검사한다. 기존 live·진행 중·완료 보관 참조·초과 알림·6시간 주기 시험을 유지한다.
- `orch::report_tests`는 실제 report collect 경계에서 완료 보고의 자동 내 보고서·안 봄과 같은 폴더 수를 확인한다. 열람 뒤 재수집·backfill·재시작·resolve의 같은 revision은 읽음 상태를 유지한다. 기존 완료 사본의 backfill도 검사한다.
- 확장 E2E21은 실제 웹 한 번 보관/해제·과거 판 선택·명시 삭제·별도 용량·Reader 제한을 Chromium/WebKit 1280/390px로 검증하고 화면을 남긴다. 진행 중 보고는 작업 탭에서 보호하고 resolve 뒤 내 보고서에 안 봄으로 이동하는지 검사한다. 완료 보고는 갱신 때만 다시 안 봄이고 source+r 두 사본 이외의 복제는 없는지 실제 폴더를 대조한다. E2E36의 작업 줄 번호 시험은 진행 중 표지로 그 상태를 명시한다.
- 참고: [Zulip message flags](https://github.com/zulip/zulip/blob/main/zerver/actions/message_flags.py)([Apache-2.0](https://github.com/zulip/zulip/blob/main/LICENSE)), [Grafana snapshot](https://github.com/grafana/grafana/blob/main/pkg/services/dashboardsnapshots/service.go)([AGPL-3.0](https://github.com/grafana/grafana/blob/main/LICENSE)), [upload-artifact retention](https://github.com/actions/upload-artifact#retention-period)([MIT](https://github.com/actions/upload-artifact/blob/main/LICENSE)). 표시 상태와 사본 분리·기한과 명시 삭제 분리만 참고하고 코드는 추가하지 않는다.
- 정본은 전체 `nice -n 15 bash scripts/ci-local.sh`, Linux strict cross와 공개 검사다. 고정 sleep·재시도·시한/기대값 완화는 쓰지 않는다.

## 머신 정보 오른쪽 패널(t-0911)

- E2E98은 실제 agent의 `MachineHealth.info` 수집을 기다리고 Chromium·WebKit ×1280·390에서 열기·닫기·Esc·데스크톱 머신 교체를 확인한다. amber·catppuccin·GitHub Dark 캡처와 패널 가로 넘침을 검사한다. 실제 수집과 구판/끊김 표시 fixture를 구분한다.
- 웹 순수 규칙은 구판의 모름, 끊긴 지표 유지, 판 비교 경고 부재와 수집 시각을 확인한다. Rust는 명령 실패/첫 줄 추출·구판 역직렬화, 실제 Host의 disconnect 및 DB 재열기 뒤 마지막 정보 유지로 확인한다.
- 명령·DOM·state 사건 완료를 기다리고 고정 sleep·재시도·시한 연장은 없다. 전체 ci-local과 공개 검사가 정본이다.

- E2E58의 가짜 CLI는 `--version`을 로그인 FIFO와 분리한다. 수정 전 fixture에 `--version`을 주고 로그인 신호를 쓰면 인증 파일을 만드는 대조로 신호 소비를 확인했다. 두 가짜 CLI의 판 조회는 FIFO writer 없이 완료돼야 하며 실제 로그인·취소·계정 선택 단언과 기존 시간 상한은 유지한다.


## macOS·Linux setup(t-0931, plan §8.477)

- `cargo test --locked -p aaa --bin aaa setup::`는 기존 Windows 질문·암호·정책·등록·되돌림과 Unix 세 종류의 질문/파일/제거, 구판 receipt 기본값, 서비스 파일 충돌·symlink·환경 거부·명령 인용을 함께 검사한다. 갱신의 부분 시작 실패에는 새 프로세스 정지 → 이전 실행 파일 복원 → 이전 서비스 재시작 순서를 단언한다.
- 실제 OS owner는 `scripts/setup-unix-e2e.py`다. 실행 파일은 Cargo 산출물의 별도 사본으로 고정하고 `--binary <사본> --kind launchd|systemd-user|systemd-system --instance aaa-test-0000-x --root /tmp/aaa-test-0000-x`로 명시 실행한다. macOS에는 `--launcher <별도 시험 launcher>`가 필요하다. 기존 운영 서비스·설정·launcher를 시험 대상으로 쓰지 않는다. Linux 시스템 시험은 폐기 가능한 VM에서만 한다.
- owner가 별도 HOME·설정·상태·작업 폴더와 loopback host를 만들고 실제 PTY에서 계획 확인·등록 코드 승인을 실행한다. native 관리자 등록/시작 → PID와 host 연결 → 상태 → 정지 후 동일 설치기로 재시작 → 제거/설정 보존 → 동명 외부 파일 거부 → Ctrl+C 무변경 → 재승인 없는 재설치 → purge/실행 파일 보존을 확인한다. 고정 sleep·재시도 대신 socket 생성·콘솔 출력·자식 종료·서비스 상태 사건과 상한으로 끝낸다.
- macOS는 시작 전/후 launchctl list·LaunchAgents 내용 해시·PATH를 저장하고 운영 aaa의 PID/상태가 같고 시험 label이 사라졌는지 비교한다. 실제 launchd는 별도 HOME의 임시 plist만 bootstrap/bootout한다. Linux는 Ubuntu 24.04 aarch64 VM에서 user/system을 각각 검사하고 systemd-analyze verify도 실행한다. 공백·`%`·`$` 경로, 로그의 일반 사용자 소유권을 포함한다. `WorkingDirectory`는 raw 경로, ExecStart는 `:`로 변수 치환을 끄며 로그를 사용자 권한으로 먼저 만든다.
- 실제 시험에는 격리 host만 쓴다. VM 내부 → 운영 host/tailnet 경로는 이 owner의 보장 범위가 아니다. VM을 지운 뒤 Lima 목록을 대조한다. 로그를 공유할 때 개인 주소·사용자 경로·장치 지문을 제거한다.
- 변경의 정본 검증은 전체 `nice -n 15 bash scripts/ci-local.sh`, Linux strict cross, 실제 Windows의 `cargo test --locked -p aaa --bin aaa setup::`다. 교차 빌드를 native 서비스 시험으로 대신하지 않으며 GitHub CI를 기다리지 않는다. Front API·Agent link·웹 E2E 번호는 유지한다.

## 머신별 workspace 자동 배정 허용(t-0935)

- `cargo test -p aaa-host workspace_access::`: 최초 기존 조합 전체 켜짐·전환 재실행/재시작 뒤 꺼짐 보존·새 머신/저장소/프로젝트 꺼짐, Reader/세션 agent 쓰기 거부·Owner 쓰기·판 충돌, 저장소/작업 폴더 자동 후보 필터·고정 머신 경고, 미설정 자동 예약의 queued·대기 이유·미기동을 검사한다.
- E2E99 `99-machine-workspaces`: 실제 host/agent에서 Chromium·WebKit의 1280/390 화면으로 Owner 체크 저장·Reader 읽기/직접 API 쓰기 거부·다른 기기 갱신·판 충돌과 경로 정본을 확인하고 화면을 캡처한다. CLI 자동 시작의 후보 없음, 큐 대기 이유, 웹 자동/고정 구분과 허용 뒤 실제 자동 시작을 확인한다. 고정 sleep·실패 재시도·시한 연장은 없다.
- 기존 m13 작업 폴더 keeper와 자동 저장소 시작 keeper, E2E47에는 실제 사용할 workspace 허용을 준비 단계에 명시한다. 로컬 repos.add의 /tmp → /private/tmp 정본화와 혼동하지 않도록 저장소 keeper는 경로@머신으로 등록한다. 기존 단언·시한은 유지한다.
- 참고·정책·새 기본값·API는 [workspace-access.md](workspace-access.md)에 적었다. 정본은 전체 로컬 ci-local·Linux strict cross clippy·공개 검사다. 착지 창 전에는 병합·push하지 않고 GitHub CI는 기다리지 않는다.
## coordinator auto mode (t-0938, plan §8.479)

- `cargo test --locked -p aaa-host auto_mode --lib`는 실제 Host·DB에서 사람만 모드 변경, 해당 프로젝트 coordinator만 자동 답,
  모드 꺼짐·중복·확인창·사용자 전용 질문 거부, 일부 답 뒤 예외 카드 유지, 추천과 다른 선택·까닭·원문 보존,
  auto-decision post의 외부 알림 큐 비발행, 재판단 요청의 원래 답 보존, host 재열기·프로젝트 이름 변경을 확인한다.
  큰 원문 75개를 페이지로 읽어 중복·누락·frame 한도와 오래된 결정 재판단도 확인한다.
- 첫 owner 실행에서 웹 기기 이름을 DB update transaction 안에서 조회해 같은 DB mutex를 다시 잡았다.
  해당 자기 시험 PID를 종료하고, 기기 이름을 transaction 전에 읽도록 고쳤다. 시한·기대값을 바꾸거나 실패를 재시도로 덮지 않는다.
- E2E100는 실제 host·agent·coordinator pane의 자식 CLI를 사용한다. 웹 두 기기 동기화·host 재시작 유지·compaction hook의
  additionalContext·일반 자동 답과 예외 카드·구조화 auto-decision post·재판단·세 끄기 경로·한 번 요약·Reader 거부를 검사한다.
  실제 자동 답 53개로 이전 페이지 읽기·옛 질문 주소·재판단을 확인한다.
  Chromium·WebKit ×1280·390px에서 입력창·선택 카드·끄기 요약을 촬영하고 가로 넘침과 reduced-motion을 검사한다.
- 완료는 요청 응답·PTY 출력·DOM·연결 사건으로 기다린다. 고정 sleep·재시도·시한 연장·단언 완화는 없다.
  실제 외부 푸시 비발행은 Rust notify 큐 owner로 검사하며 UI 알림 개수로 추정하지 않는다.
- 참고한 herdr-web-ui Composer/PromptCard(aa624cb, MIT), herdr-projects coordinator 안내·진행 hook(v0.2.35, MIT)의
  기존 이식 경계를 유지한다. 전체 ci-local·Linux strict cross·공개 검사를 수행하며 GitHub CI를 기다리지 않는다.

## E2E 세 자리 번호 수집(t-0943)

- 첫 세 자리 E2E100을 착지에 배정한 뒤 전체 gate의 파일 규칙 검사에서 RED를 확인했다. runner discovery도 두 자리만 받던 원인이므로 파일 규칙과 수집을 함께 두 자리 이상으로 바꾼다. 목록·단일 worker 실행·요약·시간 기록과 patch 파일 목록은 숫자 순을 따른다. 기존 LPT 배치와 단계 분할은 유지한다.
- `e2e-layout.test.mjs`는 실제 runner의 `--list`에서 디렉터리의 모든 파일이 정확히 한 번 수집되고 E2E99 바로 다음에 E2E100이 오는지 검사한다. 기존 파일 형식·단계 이름 중복·harness export·duration 소유 검사는 유지한다. 시험을 제외하거나 번호를 재사용하지 않는다. 전체 gate는 새 수리 SHA에서 처음부터 한 번 실행한다.

## main CI 누락 실패 수리(t-0945)

- run 39992760202(6c76bffa)의 실패는 E2E 목록 `Set(95)` 대 `Set(102)`다. Vite의 `Build failed in 9ms`는 이전 dist 보존을 확인하는 음성 시험이며 그 시험은 통과했다. `--list` 직후 `process.exit(0)`이 pipe의 미전송 출력을 버리는 경로를 없애고 자연 종료한다. `e2e-layout.test.mjs`는 실제 runner 사본과 400개·약 1.2MB 목록으로 끝 이름·단계까지 비교한다. 구판 사본은 UTF-8 중간에서도 잘리고 400개 중 일부만 반환하는 RED를 재현한다. 실제 전체 파일 수집·숫자 정렬 단언을 유지한다.
- run 39988045321(6fd0ddff)은 E2E84 WebKit에서 해제된 xterm renderer의 `dimensions`를 읽었다. upstream `Viewport.reset()`의 frame과 constructor의 task는 disposable로 취소되지 않는다. 기존 한 frame 뒤 정리만으로 모든 callback 순서를 보장하지 않으므로 실제 `Viewport.syncScrollArea`·`_innerRefresh`에 upstream `Disposable._isDisposed` 수명 검사를 붙인다. `xterm-source.test.mjs`는 패치 전후 실제 메서드와 reset callback을 실행해 해제 뒤 구판 예외·수정판 무접근을 비교한다. 기존 scroll·IME 패치와 E2E84의 오류 0건 단언을 유지한다.
- run 39980858301(83ab3346)은 `diagnostic::tests::ignored_send_error_does_not_format_its_private_payload`의 로그 1건이 0건이었다. 로컬 원본 100개 실행에서는 재현하지 못했으며 당시 callsite cache 변경 순서는 남아 있지 않다. 당시에는 subscriber 생성·해제의 전역 interest 갱신 경합을 의심하여, 하나의 Dispatch를 유지하고 thread별 수집기를 썼다. 그러나 run 40041938212에서도 재발했다. t-0973이 capture 밖 첫 호출의 단일 Dispatch interest 캐시 문제를 결정적으로 재현하고 전역 시험 subscriber로 교정했다(이 문서 첫 절). 기존 8개 thread × 64건의 개수·분리와 오류 payload 비노출 단언은 유지하고 제품 로그 필터는 바꾸지 않는다.
- run 39973893610(bc98e49b)은 accounts 시험에서 DB WAL의 무작위 8자 조각 하나를 검출했다. 원본 WAL artifact가 없어 당시 실제 누출인지 정상 hex metadata와의 일치인지 소급 확정하지 않는다. 실제 로컬 인증 회전·비노출 원본 시험은 통과했다. 정상 64자리 digest와 hex nonce의 8자 일치도 같은 검출을 만드는 결정적 대조를 추가한다. accounts fixture의 nonce는 16문자·32자리·128bit를 유지한 채 hex 밖 알파벳으로 일대일 변환한다. 실제 8자 조각 25개 모두를 계속 검출하며 원문·base64·URL·JSON·DB/WAL·로그·미전송 post 검사를 유지한다. 제품 redactor나 허용 기대값은 바꾸지 않는다.
- run 39984576747(fca9e7d3)은 Windows one-line install 시험의 실제 IRM이 시험 HTTP 서버 응답을 읽다가 connection reset으로 끝났다(`http_phase=4`, executor/config 미생성). 시험 서버는 응답을 쓴 뒤 write half-close로 EOF를 보내고 peer EOF까지 수신을 비워 종료한다. 실제 PowerShell 5.1/7의 `irm | iex`·ConPTY·검토·서명 승인·설치 기록 단언을 유지한다. Windows 후보는 격리 실기에서 해당 시험을 실행한 뒤 인계한다.
- 참고 후보는 [xterm.js Viewport/Lifecycle](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)), [tracing callsite](https://github.com/tokio-rs/tracing/blob/master/tracing-core/src/callsite.rs)([MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE)), [PowerShell](https://github.com/PowerShell/PowerShell)([MIT](https://github.com/PowerShell/PowerShell/blob/master/LICENSE.txt))다. [Node process I/O](https://nodejs.org/api/process.html#a-note-on-process-io)의 자연 종료 계약도 따른다. 외부 코드·의존성은 추가하지 않는다.
- 검증은 전체 ci-local·Linux strict cross clippy·공개 검사다. 재시도·시한 확대·기대값 완화·시험 삭제·GitHub rerun/revert는 하지 않는다. API·E2E 번호는 유지한다.

## README 촬영과 E2E 목록 출력

- `web/e2e/showcase.mjs --readme`는 실제 격리 host·agent와 가짜 agent로 README 매체를 만든다. 폐기된 evaluator 호출을 제거하고 auto mode·자동화·workspace 화면을 포함한다. 화면과 보고 사건으로 준비를 확인하며 가짜 thread 완료는 FIFO로 제어한다. 고정 sleep 제거에 맞춰 `fixed-sleeps.json`의 기존 허용 3건도 제거했다.
- 새 thread의 행은 pane 준비보다 먼저 도착할 수 있다. GIF는 `thread show`의 pane과 `pane wait-output`의 실제 작업 출력을 확인한 뒤 해당 pane을 연다. 시한 연장으로 준비 경합을 숨기지 않는다.
- `e2e-layout.test.mjs`의 전체 목록 단언이 첫 검증에서 실패했다. 102개 파일 중 stdout이 64개에서 잘렸다. 원인은 `--list`의 비동기 파이프 출력 직후 `process.exit(0)`였다. 착지에서는 main의 자연 종료 수리를 유지하고 후보의 중복 write callback 수리를 제외했다. 기존 전체 파일·중복·숫자 정렬 단언을 유지한다.
- 촬영 격리·매체 목록·OSS 구성 참고는 [README 매체](readme-media.md)에 적었다. 실제 설치·서비스 변경은 이 문서 작업에서 실행하지 않는다.


## v0.1.6 공개판과 Windows setup 입구(t-0958)

- `python3 -m unittest scripts.test_release`는 같은 Cargo 판의 tag, OS별 자산·심볼·launcher 결속, 판·독립 payload SHA를 넣은 `install.ps1`과 체크섬을 검사한다.
- Windows의 `cargo test --locked -p aaa --test windows_bootstrap`은 PowerShell 5.1·7에서 실제 PE·프로세스·argv·stdio와 `irm | iex` 입구를 실행한다. 새 입구가 `setup`을 정확히 전달하고 기존 실행 파일·설정은 보존하며 `install` 전용 인자를 붙이지 않는지 확인한다. 실제 aaa의 `setup --help` 성공과 콘솔 없는 `setup --yes` 거부·설정/상태 변경 없음을 검사한다. 기존 install dry-run 검사는 기존 install 통합 시험에 남는다.
- 게시 전에는 별도 worktree·target에서 전체 `nice -n 15 bash scripts/ci-local.sh --tee`, Linux·Windows strict cross clippy, dev 의존성 기능이 없는 Windows release 빌드와 공개 검사를 수행한다. Playwright 1.64.1은 별도 경로에 격리한다.
- 게시 뒤 tag SHA·workflow 성공·전체 자산의 공개 URL 다운로드/체크섬·manifest 결속과 macOS/Windows의 `--version`을 대조한다. Windows에서는 격리 폴더의 다운로드 파일만 실행하며 설치·서비스는 변경하지 않는다. 실패한 tag는 옮기거나 지우지 않고 원문을 보존한다.

## Windows Codex 첫 신뢰·YOLO 유지(t-0953)

- `cargo test --locked -p aaa-session --lib account::tests::`는 실제 계정 설정 생성으로
  공백·한글 cwd의 신뢰 키 한 개, 재시작 동일성, coordinator → user에서 사전 신뢰 제거를 검사한다.
  Windows 수정 전에는 DOS cwd 키 조회가 `None`이라 실패하고 수정 뒤에는 통과한다. Mac에서는 같은
  설정·정책 회귀와 기존 Claude 계정 시험을 유지한다. TOML 문서 파서로 실제 저장 bytes를 읽는다.
- `scripts/test-windows-codex-onboarding.ps1`은 PowerShell 7에서 실행하며 `-Lab C:\aaa-test\t-NNNN` 안의
  후보 `-Aaa`와 실제 `-Codex` 사본만 받는다. 실행자가 서비스·예약 작업·
  Run 키·사용자 PATH·방화벽 기준 목록을 저장·대조한다. 운영 설치·서비스·인증·개인 Codex 설정·sshd는
  바꾸지 않는다. 임의 loopback 포트의 Responses fixture와 가짜 API 인증을 쓰며 유료 모델은 호출하지 않는다.
- 실제 host → agent → session-host → ConPTY → Codex에서 git worktree의 첫 brief, 같은 thread의
  새 대화 재시작, `login=false` 셸의 `Get-Content`·`git status` 명령 결과와 최종 응답을 확인한다. 개인
  PowerShell profile의 PATH 덮어쓰기·설치된 별도 shim에 기대지 않는다. 실제 rollout의
  `approval_policy=never`·`sandbox_policy=danger-full-access`와 자기 cwd 신뢰 한 개도 검사한다.
  thread에 신뢰·샌드박스·명령 승인 질문이 생기면 실패한다. 모든 자식은 숨겨 실행하며 GUI·UAC가 없다.
- 준비 로그·CLI 종료·실제 명령 결과 HTTP·pane 출력이 완료 사건이다. 고정 sleep·실패 재시도·시한 확대는
  없으며 실패 화면·원인·host/agent 로그를 전용 폴더에 남긴다. 직접 시작한 Process와 자기 session metadata의
  PID·실행 경로를 확인해 정리한다. 실행자가 결과 보존 뒤 시험 폴더도 지운다.
- 먼저 Codex 0.164.0과 herdr v0.9.5(Apache-2.0)의 CODEX_HOME·신뢰 조회·시작 흐름을 대조했다.
  Codex와 같은 dunce 1.0.5(CC0-1.0 OR MIT-0 OR Apache-2.0)를 Windows 의존성으로 사용한다.
  Responses SSE fixture의 최소 사건은 Codex의 공식 tests/common/responses.rs를 참고한다.
  제품 API·Agent link·E2E 번호는 유지하며 전체 ci-local·Linux strict cross·공개 검사 뒤 착지를 기다린다.
## 대화 다시 열기의 제한 보관(t-0957, plan §8.483)

- `node --test web/test/retainedChats.test.mjs`는 최근 3개 LRU·합계 8 MiB·활성 단독 초과 후 이탈·숨김 5분 만료와 pane 삭제·확인된 session 교체를 검사한다. 시간은 함수 인자로 주며 실시간 대기를 하지 않는다.
- E2E103은 격리 host·실제 agent·coordinator·일반 pane의 2,400행 합성 Claude 기록으로 Chromium/WebKit ×1280·390px 각각 3회 처음/할 일 왕복/받아 둔 과거 위치를 잰다. Chromium 80ms 회선 지연은 별도 대조다. 본문·row 위치 준비 뒤 두 frame을 포함하고 송신 연결 시간과 재요청 수를 따로 기록한다. `AAA_CHAT_BASELINE=1`은 구판 실행 파일의 기준 계측에만 쓰며 새 보관 단언을 생략한다. `AAA_CHAT_MEASUREMENTS=<파일>`로 수치 원자료를 보존한다.
- E2E103의 앞 쪽 commit 실패는 추가 evaluate보다 먼저 최초 오류·stack·엔진/폭·역할/회차·페이지 번호·직전 행 수/상태·최근 pane_chat 요청을 `diagnostics/retention-pages.jsonl`과 stdout에 남긴다. 5초 추가 진단이 응답하지 않아도 그 오류는 별도 기록하고 최초 오류를 그대로 던진다. 각 쪽 시작/commit도 남겨 어느 페이지에서 진행이 멈췄는지 확인한다. `retentionFailure.test.mjs`는 진단 성공과 실패 모두에서 최초 오류의 동일성·선행 기록·페이지 상태 보존을 지킨다. 기존 20초 commit 상한과 기대값은 유지한다.
- 78회차 E2E103의 `locator.evaluate`30초 오류는 앞 쪽 count/beforeState가 catch 밖에 있어 그 기록을 남기지 못했다. count·요소 조회·commit을 모두 같은 최초 오류 보존 범위에 넣고, 함수 진입/반환 표지는 부가 진단에 남긴다. 기존30초 lookup·20초 commit·5초 부가 진단과 기대값은 유지한다. Playwright1.64.1 소스상 함수 실행은 timeout0이므로 이번30초는 요소 핸들 반환 전의 대기다. 정상 계측에서 확인한 DOM.describeNode/resolveNode 등의 구체적인 실패 호출은 원본 protocol 기록이 없어 확정하지 않는다. 보강 뒤 E2E103 단일 전체36.3초 통과는 진단 경로 확인이며 원래 간헐 정지 해결이나 전체 gate 통과를 뜻하지 않는다.
- E103 Chromium의 `retention-protocol.jsonl`은 count/조회/commit 단계의 DOM·Runtime 호출명·번호·고정 분류·응답/오류·context 생성을 관측한다. Playwright1.64.1 in-process logger를 감싸며 요청 인자·본문·쿠키는 옮기지 않는다. RPC의 Promise·대기·응답을 바꾸지 않고 메모리에 쌓아 단계/페이지 회수 때 기록한다. 페이지 종료 때 응답 없는 호출도 남긴다. 기본 위치는 기존 실패 diagnostics이며 `AAA_CHAT_DIAGNOSTICS`로 단독 계측의 보존 폴더를 지정할 수 있다. 실제 Chrome의 DOM.resolveNode 응답을 사건 gate로 보류한 대조에서 원래30초 오류·이전 페이지의 함수 반환 표지·응답하는 페이지를 함께 구분했고, 페이지 종료 대조0.393초에서는 pending 호출을 보존했다. 이 통제 대조를 원래 실패 원인 재현으로 세지 않는다. 최종 관측기를 붙인 E103 단일 전체35.752초에서 함수108호출/108응답·오류0·종료 미응답0을 확인했다. 제품·기능 기대값·시한은 유지하고 원인은 여전히 미확정이다.
- E2E104는 네 pane의 LRU·hidden/inert·재검증 응답 전 같은 DOM/초안 표시·늦은 응답 버리기·숨김 HTTP/입력 0회·5분 만료·20회 왕복 뒤 수/관측기 증가·UTF-8 용량·활성 단독 초과·새 답·history/session 교체·표시 옵션·로그아웃을 검사한다. 용량 응답 fixture는 실제 host 응답의 본문 크기만 제어하고 생각 줄을 접어 DOM 비용과 구분한다. 만료는 Playwright 시계로 진행하며 실제 5분을 자지 않는다.
- 기존 E2E22·29·30·37·41·44·93의 대화 스크롤·검색·송신·재연결과 두 controller의 입력 소유를 유지한다. 보관된 DOM을 추가해도 기존 시험 표지는 활성 화면에만 달며, 보관 수는 `.term[data-pane]`로 검사한다. 재시도·시한 확대·기대값 완화 없이 요청/응답·DOM·연결·frame 사건으로 끝낸다. Playwright 시계는 첫 대화를 열기 전에 설치해 App의 만료 타이머도 소유한다.
- 처음 비교는 같은 기준 SHA·도구·fixture를 쓴다. 공유 머신의 부하·headless 휴대폰 크기와 실제 iPhone, 합성 Claude와 운영 Codex를 구분하고 80ms 목표는 실측으로 판정한다. 전체 ci-local·Linux strict cross·의존 경계·공개 검사가 완료 조건이며 착지 창 전에는 병합·push하지 않는다.

- 20회 왕복 대조에서 xterm 5.5.0 CoreBrowserService의 미등록 ScreenDprMonitor 때문에 window resize listener가 10회에서 22개, 20회에서 32개로 늘어나는 RED를 확인했다. 기존 Disposable 등록 한 줄로 monitor를 정리한다. 실제 upstream 모듈 대조는 `xterm-source.test.mjs`에서 resize와 media listener 모두를 검사하며, E2E104의 수 증가 없음 단언을 유지한다.
- 과거 위치의 캐시 복귀도 기존 두 frame 안정화 경로를 쓴다. 재검증의 최신 쪽에 기준 행이 없더라도 보관 본문에 있으면 복원을 위한 앞 쪽 읽기를 하지 않는다. agent 재시작의 새 cursor 시험(E2E93)은 보관된 옛 「처음이다」와 구분해 이번 최신 응답의 history commit 뒤 이전 쪽을 읽는다.
- E2E30은 Chromium과 WebKit 390px에서 실제 host의 제출 ACK만 보류하고 화면을 떠난다. 연결 종료 뒤 송신 정리·초안 복구와 정확한 입력 bytes 한 번을 검사한다. E2E25는 설정 복귀 뒤 실제 readonly 해제를 기다리고, E2E84는 복귀 그리기 세 frame을 제어한 시계로 실행하되 그 시간을 기존 499/500ms 예산에 포함한다. 시한·기대값을 바꾸지 않는다.
- `conversationRefresh.test.mjs`는 최신 읽기 → 앞 쪽 두 번 → 숨김 뒤 타이머 0개를 검사한다. 구판은 앞 쪽 읽기의 drain이 기존 타이머 참조를 덮어 2개를 남긴다(RED). 앞 쪽을 시작하기 전에 기존 타이머를 정리하며 기존 lane·주기는 유지한다.
- E2E104는 숨긴 동안 1280→900px·390→430px 폭 변경에서도 같은 row/offset을 검사한다. 10회·20회 왕복의 DOM·리스너·타이머를 대조하고 Chromium은 GC 완료 뒤 실제 CDP heap 수치도 기록한다. WebKit에서 같은 API가 없으면 heap은 미측정으로 남기며 DOM 수를 heap 상한으로 해석하지 않는다.

## 자동화 schema 2·공통 실행 원장(t-0939)

- `cargo test -p aaa-host --lib automation`은 v1/v2의 profiles·시각·켜짐·범위·명령 승인 해시 보존, 이관 journal 재개·충돌, 큐/receipt/원장의 같은 transaction, 다른 규칙의 같은 예정 시각 ID 격리를 검사한다. 명령 응답 저장/receipt 삽입에 각각 SQLite 실패를 주입하고 재시작해 RPC가 한 번뿐인지 확인한다. worker의 자기 실행만 처리 신고할 수 있고 다른 thread·없는 보고·다른 결과 덮쓰기는 거부한다.
- `cargo test -p aaa --test m13 routines`는 실제 로컬·원격 agent의 명령 승인·출력 변화·이관 후 같은 출력 중복 방지와 기존 안내 동작을 확인한다. 입력 조회의 intent/result를 공통 큐에 넣어도 이전 source·알림 계약을 보존한다.
- `node web/e2e/web.e2e.mjs --spec 106 --workers 1`은 실제 host에서 이관 dry-run 무변경·stale hash 거부·시각/profiles/processed 보존·notice 영수증과 후속 결과 미연결을 검증한다. Chromium과 휴대폰 WebKit에서 실행 단계 타임라인을 연다. 기존 E2E34/40/48/49/55/64도 유지한다. 고정 sleep이나 실패 재시도는 추가하지 않는다.
- FRONT_API 1.120와 `automation_results.v1` capability가 새 실행 조회/입력 신고를 구분한다. 일반 post accepted를 입력 결과 기록 성공으로 해석하지 않는다. N-1 agent에는 새 결과 variant를 보내지 않는다.

- 착지 통합에서는 t-0945가 main에 넣은 자연 종료 방식의 E2E `--list` 수리를 유지한다. 후보의 출력 callback·강제 종료 중복 수리는 제거하고 실제 전체 목록·대량 pipe 단언은 유지한다.

## E2E23의 실제 enqueue 관측 뒤 흡수(t-0962)

- 낙관적 `.is-pending`과 transcript enqueue의 점선 말은 같은 모양이다. 앞 표시만 확인하고 remove·queued_command를 쓰면 웹이 enqueue를 읽기 전에 흡수된 pasted_content만 읽어 QueueOperation 진단을 놓친다.
- 현재 submit의 trace를 잡고 host의 해당 ID `Receipt method=Some(QueueOperation)` 사건을 기존 `until`로 확인한 뒤 흡수한다. 기존 여러 줄·대기/흡수·초안·취소·중복 제출·본문 비기록 단언을 유지하며, 같은 ID의 received → delivered → QueueOperation receipt 순서도 검사한다. 기존 20초 실패 상한을 그대로 쓰며 고정 대기·재시도·시한 확대·시험 삭제는 없다.
- E2E23 전체를 새 실험실에서 10회 단독 실행한 뒤 같은 수정 SHA에서 전체 ci-local·공개 검사·양 OS strict cross clippy·production Windows release를 한 번 수행한다. 반복 도중 실패도 통과 표본으로 바꾸지 않고 보존·중단한다.
- [Playwright 사건 대기](https://playwright.dev/docs/events)와 [WebSocket 관측](https://playwright.dev/docs/api/class-websocket)(Apache-2.0)을 대조했다. 브라우저 송신만이 아니라 기존 host 진단 원장에 도착한 사건을 관측하며 외부 코드·의존성을 추가하지 않는다.

## E2E23 이미지 decode 대상 수명(t-0962)

- 이미지 단계의 71개 await 앞뒤를 기기·paste/drop/picker와 함께 진단 파일·stdout에 기록한다. decode 대상의 연결·로딩·임시 말 여부와 분리 사건도 남기며, 실패 때만 현재 이미지 배치를 읽어 사전 layout flush로 재현을 가리지 않는다.
- 같은 조건의 병렬 실험실에서 `thumb.locator("img").evaluate(img => img.decode())`에 120초 동안 멈춤을 재현했다. 대상은 임시 메시지에서 이미 분리된 이미지(`connected=false`, `pendingMessage=true`, `complete=false`)였고 실제 기록의 현재 이미지는 로드돼 있었다. submit ACK 뒤 임시 말이 실제 기록으로 바뀌기 전에 locator가 옛 노드를 잡은 시험 순서 결함이다.
- 실제 기록의 해당 메시지가 나타나는 DOM 사건을 먼저 기다리고 `.is-pending`이 없는 메시지에서 썸네일을 고른다. 화면에 스크롤해 lazy loading을 시작한 뒤 기존 decode·바이트·해시·잘림·썸네일·삭제·권한 단언을 그대로 수행한다. decode 시작 때 연결된 실제 메시지 이미지인지도 검사한다. 제품 코드는 바꾸지 않으며 시한·기대값 완화·재시도·단계 삭제는 없다.
- 수리 SHA의 E2E23 전체 10회 모두 통과 뒤 같은 SHA로 필수 gate를 처음부터 한 번 수행한다. 진단 재현의 실패와 수리 뒤 안정성 표본을 구분한다.
- [Playwright locator](https://playwright.dev/docs/api/class-locator#locator-scroll-into-view-if-needed)(Apache-2.0), [HTML 이미지 처리](https://html.spec.whatwg.org/multipage/images.html), [WebKit ImageLoader](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/loader/ImageLoader.cpp)(LGPL-2.0-or-later)를 대조했다. 외부 코드는 가져오지 않는다.
## workspace main 통계·사용량 바닥 고정(t-0961)

- `cargo test -p aaa-agent -p aaa-host workspace_metrics`: origin 정규화·fork 구별, main checkout/미push/HEAD 변경 전 실행 제외, 원장 재시작·중복 제거·두 머신·여러 프로젝트 동일 remote 묶음, 통계 전용 초기화와 다른 데이터 보존을 검사한다. Git 실행과 SQLite는 실제 경로를 사용한다. 기존 parser·비용 상한·코드 집계 시험은 유지한다.
- E2E90은 실제 host와 두 agent를 등록하고 두 clone의 SSH/HTTPS origin을 같은 저장소로 묶는다. fixture origin/main ref는 외부 Git 서버 없이 push 완료 상태를 나타낸다. thread worktree에는 별도 결과를 두어 main 수집에 포함되지 않음을 검사한다. 조회·필터·개수·그래프는 제품 API와 렌더링을 그대로 사용한다.
- Chromium/WebKit × 1280/820/390/320px × 밝음/어두움에서 URL·상세 키보드·가로 넘침·수집 오류·머신 선/점과 필터를 검사한다. 실제 등록/원장에 긴 목록 fixture를 추가하여 workspace만 스크롤되고 공통 사용량 바닥 줄은 설정 톱니 옆에 남는지 확인한다. 답할 것·작업·할 일도 같은 위치를 검사한다. 기기 크기를 재현한 headless 시험이며 실제 iPad/iPhone 실기라고 부르지 않는다. DOM·등록 완료·보고 hash·수집 갱신이 완료 사건이다.
- 참고 후보: [herdr-web-ui Sidebar](https://github.com/devswha/herdr-web-ui/blob/main/src/components/Sidebar.css)([MIT](https://github.com/devswha/herdr-web-ui/blob/main/LICENSE)), [herdr-projects](https://github.com/eliasstravik/herdr-projects)([MIT](https://github.com/eliasstravik/herdr-projects/blob/main/LICENSE)), [Recharts 다중 선 예제](https://recharts.github.io/en-US/examples/LineChartHasMultiSeries/)([MIT](https://github.com/recharts/recharts/blob/main/LICENSE)). 앞의 두 저장소에서 전용 통계/차트 모듈은 찾지 못했다. 기존 sidebar flex 구조와 Recharts의 시계열별 선·범례 구분을 참고한다. 현재 앱은 차트 라이브러리 없이 SVG를 쓰므로 새 패키지나 외부 코드는 추가하지 않는다.

- 전체 회귀에서 발견한 E2E23 경합: 낙관적 대기 말이 나타났다고 enqueue가 읽혔다고 가정해 즉시 remove를 기록하면 수신 방식이 PastedContent로 관측된다. QueueOperation 진단 사건을 확인한 뒤 remove를 기록하도록 fixture 순서를 고친다. 기존 동일 입력 ID·여러 줄·진단 비밀값 배제 단언과 한도는 유지한다.
- E2E23의 기록 썸네일은 수신 때 교체되는 낙관적 말 대신 실제 기록 말을 선택하고, 제품의 `loading=lazy`에 맞춰 먼저 화면 안으로 스크롤한 뒤 decode를 기다린다. 임시 DOM 교체와 이미지 decode/스크롤의 경합 및 화면 밖 그림의 순환 대기를 없애며, 썸네일 잘림 단언과 단계 한도는 유지한다.
## 머신 패널 배포 팝업 A안(t-0964)

- E2E35는 기존 Reader의 전송률·복구·긴 오류·전체 SHA·시작 권한 거부·재연결·알림 검사를 유지한다. 목록의 큰 카드와 성공 카드 접기 검사는 승인된 A안에 맞춰 「최근 배포」 팝업 검사로 대체한다. 설정의 배포 카드는 기존 접기를 유지한다.
- Chromium·WebKit × 390·1280px에서 배포 없음·진행·완료·복구 중·복구 완료·복구 실패·중단·host 끊김·조회 실패를 검사한다. 실행 판/대상 판 분리, host와 다름·알 수 없음·마지막 확인, 닫기·Escape·바깥 누름·초점 복귀·Tab 가두기, 휴대폰 아래 시트, SHA 줄바꿈과 움직임 줄이기를 확인한다. WS 사건과 DOM이 완료 조건이며 고정 sleep은 없다.
- `node --test web/test/rolloutStatus.test.mjs`는 머신의 모든 단계가 끝나야 완료임을 확인하고, 현재 머신·대기 머신·실패 뒤 미진행을 구별한다. 판은 `MachineView.software`만 표시하며 배포 대상으로 덮지 않는다.
- t-0963의 OSS 조사(herdr Apache-2.0, ovis RowMenu MIT, herdr-projects MIT)를 이어 쓴다. 머신별 상태·실행 판과 대상 판 분리, 앵커 아래 팝업/휴대폰 시트, 머리 요약과 행별 결과 구성을 따른다. 외부 코드나 의존성은 추가하지 않고 기존 focus trap과 native dialog를 사용한다.
- 실행 결과: 웹 유닛 540개·타입 검사·빌드·E2E35-rollout(8.2초)·공개 검사 통과. 첫 E2E는 견본이 공통 준비 머신을 없애 대기했으므로 실제 준비 머신을 보존하도록 수리했다. 이어 native dialog의 inert 해제 전 초점 복귀 실패를 확인하여 닫기 뒤 실제 opener로 복귀하도록 고쳤다. 시한·기대값은 바꾸지 않았다. iPhone 실기와 운영 배포는 이 자동 검증에 포함하지 않는다.

## 보고서 저장소 거르기(t-0991)

- `cargo test -p aaa-host --lib library_links::tests`는 실제 thread 원장·TASKS.md·보관 할 일에서 저장소를 찾는 우선순위, Windows 형태 경로의 이름 추출, 모르는 저장소, 단건과 일괄 목록 일치를 확인한다. 기존 판·보관·live·권한·삭제 시험을 유지한다.
- 웹 유닛 `reportRepos.test.mjs`는 전체·이름·정하지 않음·옛 host 응답·선택지 개수와 빈 목록을 확인한다. 타입 검사·웹 빌드도 수행한다.
- E2E108은 격리 host의 보고서 원장·보관 판·TASKS.md를 만들고 실제 API로 읽는다. Chromium/WebKit × 1280/820/390px에서 세 탭의 필터·전체/미지정·새로고침 뒤 기억·일괄 읽음의 DB 범위·저장소 왕복 뒤 선택 해제·가로 넘침을 확인한다. DB fixture는 agent 수집 thread 없이 준비한다. 브라우저 DOM·요청 완료가 사건이며 고정 sleep·재시도·시한 확대는 없다.
- 공통 칩 추출은 E2E76·77, 기존 보고서 권한·열기·보관은 E2E21, 대량 목록·더 보기는 E2E80, 모두 읽음은 E2E101로 회귀 검사한다. 변경 전후 같은 fixture의 세 폭 캡처를 보고 library에 둔다.
- 실행 결과: 웹 유닛 547개, proto TypeScript 생성 계약 2개, host 보고서 시험 29개, crate 의존 경계 2개, host/proto all-targets clippy, 웹 타입·빌드·공개 검사 통과. E2E21·76·77·80·101과 E2E108을 통과했다. 첫 E2E108은 확인창의 실제 `alertdialog` 역할을 `dialog`로 찾은 시험 선택자 오류였으며 선택자를 고쳤다. 캡처는 메뉴 DOM과 `watchSheetExits`가 만든 닫힌 shadow root의 퇴장 잔상 host 제거를 기다린다(기존 E2E86과 같은 사건). 메뉴 열림은 유한 애니메이션 종료를 기다려 닫히던 시트가 그림에 남지 않게 한다.

## 일정의 열린 일반 thread 조건(t-0993)

- `cargo test -p aaa-host --lib automation::tests`는 schema 2 일정·명령 조회의 변환·재직렬화, 0·음수·사건 조건 거부와 기존 승인 계약을 확인한다. 실제 host DB에 프로젝트별 worker·자동화·coordinator·완료·실패·보류 thread를 넣고 일정 pass→내구 큐→inbox를 실행한다. 조건 미달에는 마지막 건너뜀만 남고 inbox는 없어야 하며, 일반 thread가 생겨도 같은 차례가 부활하지 않고 다음 차례에 안내 한 건을 남긴다. 시계 대기는 기존 일정 상태를 과거 차례로 준비하는 것으로 대신한다.
- E2E55에 열린 일반 thread 최소 개수의 폼 입력→실제 host TOML 검사→검토 문구→저장→CLI show/목록→다시 열기→빈 칸 제거를 추가한다. 기존 충돌·승인·복구·휴대폰 화면 시험을 유지한다. host 응답·DOM 사건으로 끝내고 고정 sleep·재시도·시한 증가는 없다.
- 웹 폼 유닛은 일정↔명령 조회에서 조건 보존, 사건 전환·빈 칸에서 제거를 확인한다. proto 생성 계약·공개 JSON schema·웹 유닛/타입/빌드·crate 의존 검사·host/proto clippy·공개 검사를 수행한다. 실제 운영 idle-audit 설정과 배포는 이 후보의 시험에 포함하지 않는다.

- 검증 결과: 웹 유닛 548개·타입/빌드, host 전체 유닛 639개(기존 2개 제외, 자동화 71개 포함), 의존 경계 2개, proto 생성 계약 2개, E2E55 전체(14.2초), host/proto/CLI all-targets clippy·fmt·공개 검사가 통과했다. 새 host fixture는 observe의 tagged TOML 형식과 INSERT API로 바로잡았다. 처음에는 누락된 kind와 UPDATE만 하는 thread_save 때문에 실패했으며 제품 기대값·시한은 바꾸지 않았다. 전역 Playwright 1.59.1은 두고 자기 폴더에 1.64.1을 설치해 사용했다.

## workspace CI 종료 원장 회수(t-1004)

- 운영 원장과 최근 작업을 읽기만 해 대조했다. 실행 0건·코드 줄 수 1건이었고, 최근 24시간에 변경된 보존 자료에는 CI 종료 요약 파일 213개가 있었다(복사본 포함, 고유 실행 수 아님). t-1000의 네 gate 결과도 원장에 없었다. 별도 착지 worktree·외부 `--logs`·정리된 원본과 등록 checkout의 옛 HEAD 때문에 수집되지 않았다. 화면 기간·머신 필터나 DB 보존 삭제가 원인은 아니었다.
- `python3 -m unittest scripts.test_workspace_metrics scripts.test_ci_local`은 실제 CI 셸·Git으로 성공/실패 종료, 외부 로그 폴더, 출처 변경/미커밋 거부, 원문 출력 제외와 누락된 Rust 부속 결과를 확인한다. 보관 실패는 CI 종료 코드를 덮지 않는다. 새 시험도 ci-local의 Python 목록에 포함한다.
- `nice -n 15 cargo test --locked -p aaa-agent -p aaa-host --lib workspace_metrics`는 push 전 대기→같은 입력의 push 후 확정, 옛 등록 checkout에서도 origin/main 객체 집계, 후보 브랜치 시간의 main 혼입 방지, tree 불일치 진단, 실패 실행·크기 보존, 원본 삭제 후 재회수와 기존 host 중복/필터를 검사한다. 기존 구형 요약의 시작/reflog·수집 전후 checkout 검사는 유지한다. 예전 「현재 checkout이 branch면 이미 확정한 main 기록도 숨긴다」 단언만 origin/main 객체 기준으로 바꾼다.
- E2E90 마지막 단계는 제품 CI 셸과 종료 보관 스크립트를 실제 별도 worktree에서 실행한다. 검사 단계만 격리된 public 성공/실패 명령으로 준비한다. agent→host→열린 실행 표의 갱신은 실제 제품 경로다. 미push 동안 실행 수가 늘지 않고, origin/main ref 갱신 뒤 worktree·외부 원본 로그를 먼저 지워도 성공/실패 두 건과 실행 커밋·머신·새 main 코드 줄 수가 보이는지 확인한다. 보고 재회수 뒤 중복도 검사한다. 기존 Chromium/WebKit·모든 화면 폭·밝음/어두움 단언은 유지한다. ref 갱신은 외부 Git 서버 없이 push 완료 상태를 나타내며 실제 운영 UI 확인은 배포 뒤 한다.
- Playwright는 저장소 요구 판 1.64.1의 npm 패키지만 작업 폴더에 두고 기존 내부 공용 Chromium/WebKit 캐시를 사용한다. 브라우저 설치·시한 연장·실패 재시도·고정 sleep은 추가하지 않는다.


## 짧은 ID의 다섯 자리와 숫자 동치(t-1013)

- `cargo test --locked -p aaa-proto -p aaa-common -p aaa-host --lib`는 실제 t·d·r·q·a 생성기를 9999→10000→10001까지 진행하고 재시작·프로젝트별 원장·별칭·판 순서·과거 파일·정리를 확인한다. p는 기존 8자리와 9자리 경계까지 확인한다. CI 소유자 parser도 짧은 입력과 다섯/여섯 자리를 받는다. 순번을 문자열로 정렬하는 회귀를 별도 검사한다.
- 웹 유닛은 짧은 입력·0 추가·큰 정수 정밀도·존재 확인·프로젝트 중복·낱말/경로 경계·명시 링크를 확인한다. E2E112는 격리 DB의 이전 번호를 준비하고 실제 CLI로 thread·task·질문·보고·자동화를 만든다. Chromium·WebKit에서 대화 별칭 링크와 깊은 주소·새로고침·다른 프로젝트/없는 질문의 비선택을 확인한다. E2E21·52·54·55·76·77·80으로 판·할 일·자동화·원문/주소 분리의 기존 동작을 검증한다.
- 새 fixture는 질문 없는 기준 기록을 만들어야 한다. 기준 thread의 질문·작업 경로를 복제하면 조회 때 가짜 기준 기록에도 q 번호가 발급된다. 가짜 harness에 질문 보고를 명시하고 기록의 questions·보고 hash·pane·작업 경로를 비운다. DOM·CLI 응답·보고 회수 사건으로 끝내며 시한을 늘리거나 실패한 호출을 재시도하지 않는다. CI parser의 `t-004`는 새 숫자 동치 계약의 정상 입력으로 옮기고 잘못된 꼬리 `t-004x` 거부는 유지한다.
- `python3 -m unittest scripts.test_short_ids`는 host에서 읽는 시험 instance/경로의 4·5·6자리와 번호 불일치·경로 이탈 거부를 검사한다. 운영 서비스·Windows native 경로 I/O·시스템 설정을 실행하는 시험이 아니다.


## 명시 프로젝트 보고서 링크와 대화 복귀(t-1013 후속)

- t-1014의 61회차 E2E38은 `#report=demo/chatrep`에서 프로젝트를 유지한 실제 칩(`data-target="demo/chatrep"`)을 맨 이름 `chatrep`으로 찾았다. 원본 실패는 선택자 불일치이며 공용 실패 화면은 실제 rep1 대화의 증거로 쓰지 않는다.
- 제품 수정 전 별도 관측에서 WebKit 390·430px × Safari/standalone 네 조합의 실제 대상·표시·보고서 본문·뒤로 전후 pane/스크롤을 확인했다. 대상은 `demo/chatrep`, 맨 이름 선택자는 0개이며 같은 보고서 본문과 원래 위치로 돌아왔다. 파일 보기의 네 링크 방식 × 닫기/뒤로와 데스크톱 새 탭도 유지됐다.
- 제목 없는 보고서 칩의 fallback 표시까지 `demo/chatrep`으로 바뀐 것은 의도하지 않은 표시 변경이다. 프로젝트는 내부 대상에 유지하고 표시 이름은 기존 `chatrep`으로 돌린다. E2E38은 명시 프로젝트와 맨 이름 참조 모두에 정확한 대상·표시 이름·같은 보고서 HTML 제목·같은 URL·새 창 없음·뒤로의 원래 pane/읽던 줄 1px 이내를 검사한다. 기존 파일 보기 닫기/뒤로·44px 단추·내용 면적·데스크톱 새 탭 단언도 유지한다.
- 같은 가정을 전 E2E에서 찾았다. E2E53의 login-v2는 프로젝트 없는 명시 참조이고 E2E69는 질문 ID, E2E77·112는 맨 순번 ID, E2E52의 library 경로는 첨부 대상이다. 이들은 프로젝트를 버린 보고서 주소를 전제하지 않으므로 바꾸지 않는다. 단위 시험에 맨 이름·명시 프로젝트·다른 프로젝트의 같은 이름을 구별하는 사례를 둔다.
- 새 맨 이름 링크를 추가한 첫 전체 실행은 137/138 작업이 통과하고 E2E38의 연속 복귀에서 멈췄다. 관측상 URL·대화 DOM은 먼저 돌아오고 `aria-busy="true"` 동안 줄 복원이 계속된다. 이 중간에 다음 링크로 굴리면 이전 복원이 그 이동을 덮는다. 다음 입력·위치 측정은 제품의 기존 `aria-busy="false"` 완료 사건 뒤에 한다. 위치 오차·클릭·시한 기대값은 유지한다.
- 최신 origin/main을 포함한 후보에서 `ci-local.sh --only web,e2e,public`로 웹 E2E 전체를 검증한다. 시간 상한·재시도·force click·기존 입력/복귀 기대값을 바꾸지 않는다. 브라우저는 저장소의 Chromium launcher·내부 공용 캐시와 Playwright 1.64.1을 사용한다.

## 등록을 유지하는 머신 이름 변경(t-1016, plan §8.490)

- host `machine_rename::tests` 6개는 같은 TLS 등록 ID·키와 연결 generation 유지, 열린/끝난 thread 표시, 실제 예약 테이블의 승인판·선택 고정·task 원문, 프로젝트 repos·CI 조회 머신·자동화/routine·명령 승인·workspace 허용 이전을 검사한다. 지표를 이름 변경 뒤 재회수해 중복이 생기지 않는지도 확인한다. 제목·메모·명령·env·경로·ULID·CRLF를 보존하고 이름 충돌·host 자신·Reader·세션 호출·기동/dispatch 중 변경을 거부한다.
- 같은 시험은 JSON/설정 오류 뒤 파일·DB 원복과 두 방향의 중단 journal 복구, 직접 편집 보존, 구판 agent의 옛 hello가 이름을 되돌리지 않음을 확인한다. agent `machine_label_tests`는 재시작 없이 이름표를 저장하고 원래 실행 이름으로 재시작해도 이름과 장치 ID가 유지됨을 검사한다.
- E2E60은 loopback TLS의 별도 agent와 살아 있는 셸을 만든다. CLI 변경, Chromium 1280/390px의 웹 변경·이름 규칙·충돌, 파일 참조·workspace 허용 이전, 브라우저 pane WebSocket 정확히 같은 하나 유지·미전송 입력 보존·새로고침 복원·실제 PTY 입력을 검사한다. agent를 의도적으로 종료해 연결 종료 사건을 본 뒤 옛 실행 이름으로 다시 띄워 같은 pane/ID와 새 이름이 돌아오는지 확인한다. 추가 웹 기기·원격 session-host도 시험 안에서 정리해 기존 마지막 조작 기기 단언을 유지한다.
- 웹 `machineIdentity.test.mjs`는 두 터미널 초안의 이름 기반 키를 고유 pane ID로 찾아 ID 기반 키로 옮긴다. 다른 pane·새 키·모호한 옛 키는 보존한다. E2E47의 저장 키 단언도 머신 ID 기준으로 바꾸며 글 보존 기대값은 유지한다. E2E13의 단절 초안과 E2E99의 Chromium/WebKit × 1280/390px workspace 허용도 함께 확인한다.
- 실행: host 전체 유닛 655개 통과(기존 ignored 2개 유지), agent 전체 167개, 웹 유닛 566개·타입/번들, proto TypeScript 계약 2개, crate 의존 경계 2개, host/agent/proto/CLI all-targets clippy·공개 검사. 관련 E2E13·47·60·61·72·98·99 총 7파일이 통과했다. Playwright npm 1.64.1은 자기 폴더에 두고 내부 공용 브라우저 캐시를 사용했다. 브라우저 설치·고정 sleep·시한 확대·기대값 완화·실패 자동 재시도는 없다.
- 초기 새 fixture는 ULID/자동화 schema와 CLI 응답 형식을 잘못 가정해 실패했다. 정본 parser·응답에 맞춰 고쳤다. E2E60에서 추가 Controller 기기의 `/me` 응답을 `ok` 없이 읽어 정리가 빠지자 기존 마지막 Controller 검사가 실패했으며, 기기 ID의 존재 단언과 명시적 폐기를 추가했다. clippy의 중첩 if 지적도 고쳤다. 이름 변경 자체의 연결·초안·참조 이전 기대값은 유지한다. 운영 머신 이름 변경·배포·실기 UI 확인은 이 검증에 포함하지 않는다.


## 자동화 기억 probe의 원자 게시(t-1028)

- `automation_start::automation_memory_reentry_codex_after_compact_and_resume`의 fake harness가 probe JSON을 직접 쓰는 동안 Rust가 빈 파일을 읽어 파싱에 실패했다. startup·compact·resume·child를 공통 게시 함수로 바꿔 같은 폴더의 고유 임시 파일에 JSON을 쓰고 flush·close 뒤 `os.replace`한다. Rust의 기존 파일 대기·엄격한 JSON 파싱·기억 내용 단언과 시한은 그대로 둔다.
- m13의 다른 probe도 점검했다. image·rollout·thread_park·thread_stages는 명령 출력 완료 표지를 기다리고, web_publication·accounts의 교체형 JSON fixture는 임시 파일 교체를 사용한다. 이번 기억 probe와 같은 직접 JSON 게시 지점 두 곳을 함께 수리했으며 다른 시험의 입력 fixture는 바꾸지 않는다.
- 단독 Codex 시험 5회와 공통 Claude 시험 1회를 미리 정한 표본으로 실행한다. 한 번이라도 실패하면 중단한다. 이어 전체 `nice -n 15 bash scripts/ci-local.sh --tee`, 별도 공개 검사, Linux/Windows strict cross clippy와 dev 의존성 없는 Windows release 빌드를 수행한다. 고정 sleep·실패 재시도·시한 연장·기대값 완화는 없다.
- [Python `os.replace`](https://docs.python.org/3/library/os.html#os.replace)와 [`tempfile.NamedTemporaryFile`](https://docs.python.org/3/library/tempfile.html#tempfile.NamedTemporaryFile), 기존 m13 web_publication의 임시 파일 교체를 참고했다. Python 구현은 PSF License다. 외부 코드를 복사하거나 의존성을 추가하지 않았다.


## E2E104의 정상 단발 예약과 타이머 누수 구분(t-1028)

- 기준 main과 t-1021+probe 수리 조합에서 Chromium 1280/390px를 같은 진단 계측·단독 worker로 각각 사전 고정 3회 실행했고 모두 통과했다. 전체 gate의 pending timer 1→3은 단순 총수 스냅샷이었다. 왕복 단계별 등록 stack을 남기니 20번째 복귀에도 ChatFind.useDebounced 100ms, Term ResizeObserver 120ms, ConversationRefresh.drain 약 1.97초가 함께 남는 순간이 있었다. 세 번째는 검색어 debounce이며, 제품 타이머 누수의 증거는 아니었다.
- E2E104는 관측한 100/120ms 단발 예약이 실행·취소되는 사건 뒤 수량을 잰다. setTimeout 계측은 생성 stack과 callback을 남기고, queueMicrotask에서 취소→같은 사건의 재등록까지 확인한다. 다른 타이머는 기다려 숨기지 않고 즉시 스냅샷에 남긴다. 기존 timer 증가 허용치·listener/term 수·숨김 HTTP/입력·LRU·TTL·용량 단언과 단계 시한은 유지한다.
- `web/test/retentionTimers.test.mjs`는 수동 callback 사건으로 정상 3→1, callback/취소 뒤 단발 재등록, 남은 타이머 2개의 기존 증가 단언 실패, 단발 지연값으로 끝없이 재예약하는 경우의 미정착을 확인한다. 고정 sleep·시한/기대값 완화·실패 재시도는 없다. 공통 계측은 E2E104의 Chromium/WebKit 두 폭으로 확인한다.

## 웹 일괄 변경의 완료와 대량 입력(t-1031, plan §8.492)

- E2E113은 Chromium/WebKit × 1280/390px에서 원문·그림을 가진 답 500개의 삭제, 보고서 601개의 보관·내 보고서/보관함 모두 읽음·영구 삭제, 사본 500개 정리·공유 링크 500개 무효화를 실제 host로 실행한다. 작업 보고 탭은 안 봄 대상이 없어 버튼 비활성도 확인한다. 답은 창을 연 뒤 생긴 1개를 보존하며, 지울 때 처음 50개 밖의 기록·원문·그림도 제거한다.
- E2E114는 같은 네 조합에서 inbox 500개 완료·새 항목 보존, 파일 500개 업로드·게시·목록 갱신, 끝난 자리 기록 500개와 실제 Git worktree 하나의 정리를 확인한다. inbox는 현재 앱 라우팅에서 숨겨져 있어 실제 `InboxPanel`을 시험용 Vite 진입점으로 빌드하고 동일 격리 host의 API·쿠키로 연다. 서버 변경은 가짜 응답으로 대체하지 않는다. 자리 499개는 이미 사라진 worktree의 잔여 기록이고 1개는 실제 worktree다.
- 쓰기 응답을 실제 host에서 받은 뒤 브라우저 전달만 보류해 처리 중 표시·비활성 버튼·Escape 보호·frame/키 사건을 확인한다. 응답 해제 뒤 창 닫힘·목록과 DB/파일을 대조한다. 삭제 두 번째 묶음에 503을 주는 시험은 첫 500개의 실제 완료·101개 잔여·원인 표시·자동 재시도 없음·사람이 다시 실행할 때 101개만 전송함을 확인한다.
- `bulkAction.test.mjs`는 1201개의 500/500/201 순차 처리, 시작 뒤 추가된 항목 제외, 부분 실패 원인·완료 수와 다음 요청 중단, 빈 대상의 무요청을 검사한다. 요청 한도를 바꾸거나 병렬 전송으로 서버에 부하를 몰지 않는다.
- 수정 전 대조는 500개의 DB·원문·그림 삭제와 0개 목록 반영 뒤에도 확인 창이 한 개 남는 단언 실패다. 제품 원인은 성공 때 닫기를 호출하지 않는 공통 확인 창과 AsksPanel의 누락이다. 확인 성공이 컴포넌트 제거에 우연히 의존하지 않게 고친다.
- 시험 준비의 작업 보고 안 봄 가정은 실제 `to_user` 정책에 맞췄다. 휴대폰은 확인 창의 history 복귀가 목록 서랍을 닫으므로 다시 조작할 때 기존 `inList`를 쓴다. 초기 pane 자동 선택이 끝나기 전에 서랍을 여는 경합도 활성 terminal의 DOM 사건으로 구분한다. 키 관측기는 설치 완료 뒤 실제 키를 보내며, 전달 gate 오류가 원래 실패를 덮지 않게 정리한다. Vite 시험 진입점도 제품과 같은 production React 설정을 쓴다. 고정 sleep·실패 자동 재시도·시한 연장은 추가하지 않는다.

- 첫 전체 회귀에서 E2E26은 50개씩 더 보는 새 절차 없이 79번째 답을 찾았다. 「더 보기」 후 79개 표시를 확인하고 기존 마지막 항목·주소·원문·스크롤 단언을 유지한다. 시험을 삭제하거나 마지막 항목 기대값을 낮추지 않는다.
- E2E113은 조합 사이 삭제된 보고 판 이력을 보존한다. 1,803개 삭제 이력과 601개 현재 보고에서 용량 조회가 약 29.7초 걸려 다음 쓰기 응답이 시한을 넘는 문제를 재현했다. 원본별 복합 색인 추가 뒤 동일 계측의 최대 용량 조회는 약 0.97초, 전체 시나리오는 약 6.7초였다(격리 실험실 관측이며 성능 보장은 아니다). `source_index_upgrades_history_without_changing_visible_versions`는 색인이 없는 기존 DB 재개방·반복 기동·삭제/미완성 제외·프로젝트 분리·판 순서·행 보존과 실제 조회 계획의 원본 색인 사용을 검사한다.
- 자동화 대량 시험은 결과 불명 묶음 500건과 남은 입력 500건을 준비한다. 버리기는 둘을 정리하고 계속 보내기는 결과 불명만 확인 처리하며 남은 500건을 보존한다. 꺼진 규칙으로 ticker 소비를 분리한다. 이전 형식 500개도 실제 host로 계획·변환하며 원문 보존 사본, 새 규칙, 창 자동 닫힘과 갱신 목록의 보존 위치를 확인한다. 기존 E2E40의 성공 결과 단언은 닫힌 창 대신 목록의 완료 안내를 읽으며 오류·계획 hash·원문/두 판 보존·Reader 단언은 유지한다.

- 자동화 입력 fixture는 현재 `schema=2`를 쓴다. 초기 구형 TOML fixture 네 개도 변환 계획에 들어가 500개 규칙 단언이 실패했다. 실제 미리보기 응답의 504개를 확인한 뒤 큐 자료를 현재 형식으로 고쳤으며, 500개 변환·원문 보존 기대값은 유지한다.

- 자동화 완료 안내는 `inList`에 안내 항목 자체를 넘겨 본다. 휴대폰에서는 확인 창 history 복귀로 닫힌 서랍을 다시 열고, 데스크톱에서는 이미 보이는 활동 단추를 다시 눌러 목록을 접지 않는다. 완료 안내의 실제 표시 단언은 유지한다.

- 검증 결과: 웹 유닛 573개·타입/번들, host 유닛 659개(기존 ignored 2개)·all-targets clippy, crate 의존 경계 2개·fmt·공개 검사 통과. 색인 수리 뒤 전체 웹 E2E 114파일/140작업이 통과했다. 이후 자동화 변환의 최종 UI와 추가 대량 시나리오는 최종 빌드로 E2E40·114 전체를 실행해 통과했다.

- t-1035 전체 gate 실패 후 자리 정리는 실제 500개·실제 worktree 삭제·20초 host 응답 단언을 유지하고 계측만 추가한다. agent 실제 Git 시험은 부재/존재/등록된 없는 자리/남은 branch/재생성/외장 root 단절/symlink/미완료 재잠금 journal을 검사한다. host 시험은 601개의 500/101 묶음과 실제 대상 개별 정리, 구형 agent·probe 오류의 기존 경로, 확인 중 재개된 thread 보존, DB 저장 실패의 원인·미완료 건수를 확인한다.
- 첫 후속 전체 부하에서 정리 응답은 1.3/2.7초였으나 다음 WebKit fixture의 외부 DB writer가 267개 입력 뒤 잠금 충돌했다. 자리 fixture만 격리 host 종료 사건 뒤 한 transaction으로 넣고, host 로그의 listen·agent 연결 사건 뒤 실제 UI 정리를 실행한다. host 처리 시한과 정리 결과 단언은 유지하며 DB busy 재시도/시한 증가는 없다.
- 같은 부하에서 확인 창 취소의 지연된 popstate가 휴대폰 목록을 닫아 다음 누름과 경합했다. 구 UI에서 실제 popstate와 drawer transition 완료 사건 뒤 목록 표시 단언 실패를 재현했다. 내부 sheet 복귀를 구분한 제품 수리 뒤 같은 단언을 유지하며 실제 Back 회귀도 전체 E2E에 포함한다.

- 후속 최종 검증은 `ci-local` 전체를 기본 E2E 10 worker·Rust 시험·clippy 동시 실행으로 수행했다. 361초에 전 단계 통과: 웹 유닛 573개, Rust 1,342개(기존 ignored 6개), Python 56개(기존 skip 2개), E2E 114파일/140작업. Linux 대상 strict all-targets clippy도 통과했다. 전체 load1은 4.1~34.5, 자리 정리 완료 근처는 21.2~26.5였다.
- 같은 실행에서 자리 500개 응답은 Chromium 1280/390px 4.1/4.4초, WebKit 1280/390px 2.8/6.0초였다. 각 요청은 부재 499개를 묶음 확인하고 실제 worktree 1개는 기존 안전 정리로 삭제했다. 다른 일괄 요청도 통과했다: 보고서 500개 삭제 최대 11.2초, 500개 자동화 변환 최대 12.9초. 이는 해당 전체 gate의 관측값이며 시한·대상 수·최종 DB/파일/화면 단언은 유지했다.

## 보고서 500개 파일 전송 비용(t-1045)

- E2E114의 `library_upload_publish` gate는 기존대로 첫 파일 POST 전부터 전송+게시를 함께 재며 20초 상한·500개 요청·파일/목록·진행·창 닫힘 단언을 유지한다. 500개 순차 POST의 1280/390px 단독 비교는 Chromium 928/1284ms, WebKit 1279/1633ms였다. 전체 10 worker 부하 재현과 구별한다.
- `AAA_LOG=aaa=info,aaa_upload=debug`는 파일별 검증·잠금·용량 스캔·쓰기/rename/표지 비용, HTTP 본문 읽기·요청 경과와 캐시 적중 여부를 기록한다. 경로·본문은 기록하지 않는다. E2E114는 `bulk-upload`에 500개 요청의 합계/최대와 전송+게시 경과·host 구간 합계를 남긴다. 동시 요청 시간의 합계는 전체 경과와 다르다. 파일 업로드 쓰기 경로에는 fsync가 없고, 게시의 manifest 원자 쓰기는 기존대로 유지한다.
- 기준 WebKit 390px의 파일 요청 시간 합계는 1040ms, 서버 스캔 255ms·쓰기 112ms·검증 5ms였다. 브라우저는 네 파일까지만 전송하고, 업로드 폴더의 writer는 기존 서버 잠금을 공유한다. 마지막 업로드 하나의 파일 수/용량만 잠금 안에서 갱신해 파일마다 전체 폴더를 읽지 않는다. 다른 업로드·host·재시작·쓰기 실패 때 다시 스캔하며 게시 때 실제 파일 수 검사는 유지한다. 요청의 파일 I/O·잠금 대기는 `spawn_blocking`에서 처리한다.
- 웹 단위 시험은 역순 완료·첫 실패 뒤 새 요청 중단·진행 중 요청 완료 대기·성공 경로 보존·명시적 재개·게시만 재개를 검사한다. host 단위 시험은 0바이트 교체·쓰기 실패 뒤 재스캔·다른 업로드로 전환·1000개 상한·파일별 크기 상한을 검사한다. E2E114의 추가 부분 실패 단계는 Chromium 1280px·WebKit 390px에서 네 요청 중 하나를 503으로 거부하고 나머지를 보류한다. 진행 중 닫기 금지·성공 셋 보존·자동 재시도 없음·명시적 재개 뒤 경로별 요청 횟수와 실제 본문을 확인한다.
- 부분 실패 fixture 초안의 WebKit `route.fetch()`는 File 본문을 비워 첫 성공 셋만 0바이트로 저장했다. 가로채지 않은 나머지 파일은 원문이 보존됐고 Chromium도 원문이 보존됐다. fixture를 원래 브라우저 요청의 보류/`fallback()`으로 바꿔 실제 바이트를 유지한다. 시한·기대값·제품 전송 형식은 바꾸지 않는다.
- 참고 후보는 [Uppy XHR](https://uppy.io/docs/xhr-upload/)([MIT](https://github.com/transloadit/uppy/blob/main/LICENSE))의 동시 전송 상한/파일별 결과와 [p-limit](https://github.com/sindresorhus/p-limit)([MIT](https://github.com/sindresorhus/p-limit/blob/main/license))의 유한 worker다. 방식만 참고하며 외부 코드·의존성·자동 재시도는 추가하지 않는다.
- 서버 캐시까지 적용한 단일 E2E114 업로드/부분 실패 실행은 네 조합을 통과했고, WebKit 390px 전송+게시는 1046ms·스캔 합계 30930µs였다. 각 조합은 500개 중 499개 요청에서 용량 캐시가 적중했다. 실행별 외부 부하가 같지 않으므로 이 수치는 원인·개선 계측이며 전체 gate의 통과 근거로 대신 쓰지 않는다.
- 최종 `a36bd156` 전체 `AAA_E2E_WORKERS=10` ci-local은 358초에 모든 단계가 통과했다. 시작 load1/5/15는 6.70/16.30/18.21, 실행 중 표본 load1 최대는 35.03이었다. 웹 유닛 583개, Rust 1,347개(기존 ignored 6), Python 56개(기존 skip 2), 웹 E2E 114파일/140작업(295초)이 통과했다. 전체 gate의 500개 전송+게시는 Chromium 1280/390px 1265/1800ms, WebKit 1280/390px 1242/2889ms였고 각 조합에서 요청 500회·캐시 적중 499회를 확인했다. 원래 20초 상한과 모든 완료·실패·재개 단언을 유지했다. Linux x86_64 strict cross clippy도 통과했다.

## E2E114 inbox 전용 진입과 초기 앱 분리(t-1056)

- CI 원문은 WebKit 390px의 초기 앱 `terminalInflate.worker` 접근 제어 오류다. 앞선 세 조합의 500개 완료는 통과했으며 전용 inbox 번들에는 해당 Worker가 없다. 기존 macOS·worker 2 대조는 통과했고 과거 오류의 정확한 발생 시각은 기록되지 않아 문서 전환과의 인과는 미확정이다.
- 초기 앱과 Worker는 실제 host가 자산을 응답하므로 fixture 전용 원인이라고 단정하지 않는다. WebKit의 실제 host 터미널에 접근 거부를 주입해 CI와 같은 접근 제어 pageerror 문구를 재현했다. 이는 Worker error의 기본 전파를 취소해도 발생했고 fetch 접근 거부도 같은 보안 오류를 남겼다. 제품은 Vite의 `?worker&inline`으로 작은 Worker를 앱에 함께 넣어 별도 HTTP 로드를 없앤다. 생성 실패는 identity로 복구하고 실행 오류는 기본 전파를 취소하며, 준비 뒤 실패는 기존대로 연결을 중단한다. 실패 단계·원인·자산 경로·줄/칸을 진단 로그에 남기고 종료 뒤 늦은 오류도 전파하지 않는다.
- inbox fixture는 실제 GUI pairing을 기존 `#tasks`에서 수행해 떠날 초기 문서의 터미널 기동을 없앤다. 초기 앱과 전용 문서의 오류를 각각 검사하고 터미널 Worker 요청·기동이 없다는 단언을 추가한다. Chromium/WebKit × 1280/390px의 500개 완료·새 항목 하나 보존·단일 요청·DB/화면 검사와 나머지 대량 동작은 유지한다.
- `BulkInboxDiagnostics`는 pairing 전에 관측을 시작하고 문서 전환·자산 응답/실패·Worker·전체 pageerror stack에 단계와 시각을 붙인다. 실패 시 context를 닫기 전에 JSON·본문·PNG를 `diagnostics/bulk-inbox`에 저장한다. 공통 harness가 이를 기존 CI artifact로 복사한다. 요청 본문·cookie·짝짓기 code·URL query는 수집하지 않는다.
- `node --test web/test/bulk-inbox-diagnostics.test.mjs web/test/bulk-fixture.test.mjs`는 두 문서의 오류·전체 stack·자산 실패·화면 보존과 기존 DB fixture 계약을 검사한다. 관련 브라우저 회귀는 `node web/e2e/web.e2e.mjs --spec 97,113,114 --workers 2`다. 고정 sleep·자동 재시도·시한 증가·기대값 완화는 없다.
- E2E97에는 Chromium/WebKit × 1280/390px에서 별도 Worker 자산 요청을 접근 거부하도록 두어도 정상 gzip 협상이 되는지 검사한다. 생성 거부·Worker 실행 실패도 각각 주입해 identity 연결을 확인한다. 세 경우 모두 실제 host 터미널 표시·실제 PTY 입력/출력·WS 한 번·외부 Worker 요청 0회·실패 진단·pageerror 없음을 검사한다. 기존 성공 Worker의 gzip·ACK·history·재연결 경로도 그대로 남긴다. `terminalCompression.test.mjs`는 준비 전 복구, 해제 중 실패의 출력 폐기/중단, 종료 뒤 늦은 오류의 전파 차단을 검사한다. 이는 headless WebKit 회귀이며 실제 iPhone Safari 실기를 뜻하지 않는다.
- 앱의 실제 응답에서 `worker-src 'self' blob:`과 기존 `script-src 'self'`를 함께 확인한다. 보고서의 별도 sandbox CSP는 바꾸지 않는다. 입력 검사는 명령 원문에는 없는 셸 계산 결과를 짧은 고유 표지와 함께 기다려 휴대폰의 visible 출력 행 경계에 걸리지 않게 한다.
- 먼저 [Playwright pageerror](https://playwright.dev/docs/api/class-page#page-event-page-error)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)), [Vite inline Worker](https://vite.dev/guide/features.html#web-workers)([MIT](https://github.com/vitejs/vite/blob/main/LICENSE)), [WebKit Worker loader](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/workers/Worker.cpp)(해당 파일 BSD-2-Clause)를 대조했다. 기존 Vite의 Worker 내장 기능과 사건 기록 방식을 쓰며 외부 코드·의존성을 추가하지 않았다.

## E2E26 권한 응답 뒤 목록 끝 유지(t-1056)

- 기준 main e58aa62f와 Worker inline/CSP 후보 3085d4f7에 같은 실제 host·Playwright 1.64.1·WebKit desktop·worker 10을 적용했다. `/api/v1/me`의 실제 Response 전달을 끝 스크롤 대입 사건까지 붙잡았다가 놓으면 양쪽 10/10 모두 원래 `20000ms 안에 목록 끝 도착` 오류를 냈다. 응답 본문·헤더·CSP·Service Worker는 유지한다. 원래 실패의 selector/좌표는 없지만 격리 DB의 마지막 WebKit 보고서 열람은 할 일/완료 구간까지 도달했음을 보인다. 이 재현은 Worker 변경과 독립적인 기존 제품 결함이다.
- 권한 전 목록은 scrollTop 2021/clientHeight 701/scrollHeight 2722로 끝이었다. 편집 도구 반영 뒤 콘텐츠가 22px 늘면서 anchoring은 scrollTop 2043으로 따라가지만, 보기 높이가 659px로 줄어 끝에 42px가 남는다. `TasksView`는 /me를 적용하기 직전 끝 여부를 기억하고 `useLayoutEffect`에서 도구 줄을 그린 뒤 실제 끝을 보정한다. 중간은 첫 온전한 행의 상대 위치를 보존하고, 숨은 목록·교체된 요소는 보정하지 않는다. 보정 전 목록의 새 사용자 입력은 예약을 취소한다.
- 기존 E2E26의 79행 준비·native wheel·끝 좌표·마지막 행 hit test·선택·완료 다음 페이지·두 엔진/두 폭과 시한을 유지한다. fixture가 권한 응답까지 먼저 기다려 제품 경합을 피하지 않는다. 추가 회귀는 Chromium/WebKit × 1280/390px × 끝/중간의 8조합이다. `deferTaskCapabilities`는 pending인 TasksView가 요청한 실제 /me Response의 전달만 붙잡고, 끝 스크롤 뒤 또는 중간의 행·offset을 읽은 뒤 사건으로 전달을 놓는다. 끝은 새 끝에 남고 중간은 같은 행/offset을 유지해야 한다. 수리 전 binary에 새 전체 E2E26을 적용하면 첫 권한 지연 desktop 단계부터 42px 차이로 실패한다.
- `PanelScrollDiagnostics`는 실패할 때 context 종료 전에 대상 selector·scrollTop/clientHeight/scrollHeight·휠/scroll/scrollend·초기 geometry·요소 ID·행 수·폰트·숨김·PNG를 남긴다. 정상 시험의 evaluate 왕복을 늘리거나 스크롤 대입 뒤 레이아웃을 추가로 강제하지 않는다. `AAA_E2E_PANEL_TRACE=1`이면 성공 경로도 별도 artifact에 남겨 대조한다. helper만 고른 부분 실행은 앞 spec의 프로젝트 fixture가 없어 실행할 수 있으므로 새 회귀의 red/green은 전체 E2E26으로 검사한다.
- 관련 확인은 `node web/e2e/web.e2e.mjs --spec 26,97,114 --workers 2`다. 고정 sleep·실패 재시도·시한 증가·기대값 완화·시험 제외는 없다. 전체 ci-local은 이 회귀 뒤 별도 차례를 받아 처음부터 실행한다.
- [TanStack Virtual end anchor](https://github.com/TanStack/virtual/blob/main/docs/api/virtualizer.md)([MIT](https://github.com/TanStack/virtual/blob/main/LICENSE))의 변경 전 끝 여부/변경 후 보정, 기존 Chat의 끝/중간 위치 분리를 참고했다. [Playwright wheel](https://playwright.dev/docs/api/class-mouse#mouse-wheel)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))과 [WPT scroll_support](https://github.com/web-platform-tests/wpt/blob/master/dom/events/scrolling/scroll_support.js)([BSD-3-Clause](https://github.com/web-platform-tests/wpt/blob/master/LICENSE.md))도 조사했으나 재현하지 못한 native wheel 겹침은 수리 근거로 쓰지 않았다. 외부 코드·의존성을 추가하지 않는다.

## 프로젝트 공통 검증 원장 (t-1051)

- `python3 -m unittest scripts.test_workspace_record scripts.test_workspace_metrics`는 실제 Git으로 rc·엔진·pt의 동일 main 반복 실행, 성공/실패, full·coverage 모드, 오래된 부속 결과 제외, 집계 필드만 보관, tracked 변경 거부와 종료 코드 보존을 확인한다. game 저장소에도 같은 writer 회귀를 두고 공식 검증에 포함한다.
- rc의 `node --test scripts/verify.test.mjs`, 엔진의 `node --test scripts/verify_engine.test.mjs`는 기존 gate stub과 실제 Git·writer로 실제 검증 셸의 성공/실패 종료 원장을 확인한다. gate·시간 상한·성공 기대값은 바꾸지 않는다.
- `nice -n 15 cargo test --profile verify --locked -p aaa-agent -p aaa-host --lib workspace_metrics`는 세 runner의 반복 main 기록·기존 CI 호환·runtime 개수/시간·원본 정리 후 보존·출처 거부와 repo 없는 보고의 머신별 전체 회수 및 일부 실패 후 진행을 확인한다.
- E2E90은 실제 공용 writer를 별도 worktree에서 rc·엔진·pt runner로 두 번 실행하고 worktree/target을 삭제한다. 전용 thread 없이 기존 두 머신 보고만 바꿔 실제 agent→host 원장→workspace 웹 표에 각 두 행과 테스트/E2E 수가 보이는지 검사한다. 이는 검증 실행 자체를 대체하지 않는 collector fixture이며 실제 게임 전체 검증 결과와 구분한다. 기존 두 브라우저·여러 화면·CI 출처 회귀는 유지한다. 고정 sleep·재시도·시한 증가는 없다.

## 같은 tree 착지 병합의 검증 귀속 (t-1065)

- 원인은 후보를 검증하고 같은 tree로 `--no-ff` 착지해도, 종료 원장의 커밋이 main 첫 부모 계보에 없어 회수를 기다린 데 있다. `cargo test --profile verify --locked -p aaa-agent --lib workspace_metrics`의 새 회귀 하나가 네 runner의 성공·실패를 같은 tree의 직접 둘째 부모에서 main 병합으로 귀속하는 계약을 지킨다. 미push 병합, 같은 tree의 무관한 커밋·둘째 부모의 조상은 제외하며 재회수·원본 정리 뒤 같은 ID/시간/출처를 보존한다. 기존 다른 tree 병합·잘못된 provenance 거부와 parser 캐시 이행 시험을 확장한다. 새 시험은 이 누락과 과도한 귀속을 함께 막기 위해 필요하다.
- E2E90의 기존 게임 fixture를 후보 커밋 검증→같은 tree 병합→push 참조 갱신→worktree 삭제로 바꾼다. 기존 보고 회수와 두 머신을 통해 세 runner의 성공·실패 두 행, 실제 경과, 원래 검증 커밋의 웹 상세를 확인한다. rc·pt는 Chromium/WebKit 각각 1280/390px 실행 기록 표를 headless 촬영한다. DOM·보고 회수 사건으로 완료하며 고정 sleep·재시도·시한 확대 없이 기존 전체 조합도 유지한다. 기존 main 반복 검증 계약은 agent 회귀가 계속 검사한다.
- 참고 후보: [Git commit/tree 비교](https://git-scm.com/docs/git-diff-tree)([GPL-2.0](https://github.com/git/git/blob/master/COPYING)), [libgit2 이력 순회 예제](https://github.com/libgit2/libgit2/blob/main/examples/log.c)(예제 CC0, 라이브러리 [GPL-2.0+linking exception](https://github.com/libgit2/libgit2/blob/main/COPYING)). tree와 직접 부모를 실제 객체로 확인하는 원칙을 참고했고 외부 코드·의존성은 추가하지 않는다.

## E2E53 직접 답의 native input·부모 갱신 경합 (t-1065)

- 후보 17263e91의 첫 전체 gate는 388초에 E2E53 WebKit 1280px에서 실패했다. host 응답 20초 오류였으나 실제 `thread_answer` 요청은 0건이었다. 입력 capture의 `이전 답`이 1ms 뒤 빈 글로 돌아왔고 보내기 단추는 계속 비활성이었다. host 로그에도 해당 답의 pane 전달은 없었다. E2E90은 같은 전체 gate에서 103.8초에 통과했다.
- 변경 전 main e58aa62f의 웹·host 소스는 후보와 같으며, 별도 Git 사본·별도 target에서 빌드했다. 같은 E2E53 단계 순서의 일반 실행은 90.6초에 통과했다. 이 통과를 원본 실패의 해소로 보지 않는다. 기존 대화 입력 경합(t-0969)을 참고해 native input의 root capture 뒤 실제 활동 막대 click으로 부모 갱신을 끼웠다. DOM 값·React state·host 응답을 직접 고치지 않은 이 순서에서 기준 main의 Chromium·WebKit 1280px와 후보의 수정 전 실행 파일이 모두 빈 입력·답 요청 0건으로 실패했다. 같은 controlled 값 복원 경로를 제어한 재현이며, 원본 실패 때 끼어든 모든 commit의 발생원을 소급 확정하지 않는다. mouseup 선택 사건만 끼운 별도 대조는 통과했다.
- 질문 입력도 대화 입력과 같이 window native input capture에서 새 글을 먼저 state에 보관한다. 기존 bubble onChange·보고별 카드 교체·늦은 답 응답 처리는 유지한다. 숨김·해제·보기 권한·전송 중에는 listener를 해제하며 자기 입력칸 사건만 받는다. 이는 native input과 bubble 사이 부모 commit으로 사용자 글이 사라지는 제품 결함을 막는다.
- 새 시험 파일을 만들지 않고 기존 E2E53의 Chromium/WebKit × 1280/390px에 위 순서를 고정한다. 처음 채운 입력·보내기 활성·같은 DOM·부모 갱신 정확히 한 번을 즉시 확인한 뒤 원래 이전 답 응답 보류→새 보고 입력→늦은 응답→각 보고 정확히 한 번 전달 단언을 실행한다. 이 검사는 실패의 실제 경계인 사용자 입력 유실을 막기 위해 필요하다. 실패 자료는 보고 hash·질문 ID·입력/갱신 순서를 함께 보존한다. 재시도·시한 확대·기대값 완화·입력 재주입은 없다.
- OSS 후보는 [React ChangeEventPlugin](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/plugins/ChangeEventPlugin.js)·[UpdateBatching](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/events/ReactDOMUpdateBatching.js)([MIT](https://github.com/facebook/react/blob/main/LICENSE))와 [Playwright fill의 입력 경계](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/server/dom.ts)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))다. 기존 저장소의 ChatComposer 보호 방식을 참고하며 외부 코드·의존성을 추가하지 않는다.


## harness별 계정 자동 배정과 사용량 불명 실행(t-1107, §8.500)

- `account_pool::tests`는 미관측·오래됨·조회 실패·리셋·누락·잘못된 값·충돌에서도 반복 시작, 5시간 누락+주간10%, 단독 창100% 제외, 실제 진행 수/최근 배정과 옛 그룹·모델 무시, 동시 여덟 시작의 4/4 분산·영수증 재전송을 확인한다. 필요 이유는 원래 사용량 부족을 모르는 상태가 시작을 막는 회귀와 동시 요청 편향을 막기 위해서다. 이전 원문 보존·한 번 이전·꺼 둔 계정 보존·진행 중 계정 불변도 확인한다.
- 같은 owner에서 이미지 crash 복구/중복 실행 금지·인증 정리와 종료 이력·대기 취소·Reader·설정 revision 계약을 유지한다. 옛 예상 %·공유 그룹/허용 모델·프로젝트 참여·미관측 첫 실행 한 번 제한을 요구하던 시험은 새 사용자 정책으로 대체한다. 시한이나 기대값을 완화해 실패를 숨기는 변경이 아니다.
- 자동화 `pooled_automation_starts_with_unknown_or_week_only_usage_and_retains_real_low_usage`는 실제 host/DB/frame owner에서 사용량 조회 잠금을 쥔 채 미관측/주간 단독 관측으로 실제 시작 한 번·입력 회수·영수증 중복 방지를 검사한다. 알려진 문턱 미달은 입력을 보존하고 회복 뒤 한 번 시작한다. 선택 transaction의 문턱은 pool owner에서도 확인한다. 필요한 이유는 배정 전 계정 미확정으로 rc-verify가 영구 대기한 실제 경로를 지키기 위해서다.
- `repeated_usage_waits_survive_inbox_cleanup_and_restart_until_reason_or_input_changes`는 실제 자동화 대기 네 번과 매번 inbox 처리를 수행한다. 안내 한 건과 입력 보존·host 재기동 뒤 재안내 없음, 이유/입력 변화·해제/재차단을 확인한다. 필요 이유는 ticker마다 같은 안내가 다시 쌓이는 회귀를 막기 위해서다.
- 기존 E2E81에서 Chromium/WebKit × 1280/390px 실제 계정 행 스위치·그룹/모델/프로젝트 제거·가로 넘침을 검사한다. 실제 host/agent/PTY의 같은 harness 분산·미관측 반복 시작·5시간 누락+주간10%/100%·다른 harness 기본 계정 fallback·영수증·재기동·할 일 대기/취소·Reader와 인증 비노출도 확인한다. E2E58은 t-1100의 이름 변경 UI를 유지하며 host 회귀에서 v2 사용 설정 동반 이전도 확인한다.
- 부분 명령은 `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib`와 `NODE_PATH=<작업 폴더 Playwright 1.64.1> nice -n 15 node web/e2e/web.e2e.mjs --spec 58,65,78,81 --workers 1`이다. 웹 빌드·유닛, proto 생성 일치·의존 규칙도 검사한다. 공용 브라우저 캐시만 쓰며 고정 sleep·재시도·시한 확대는 없다. 전체 ci-local은 coordinator가 준 우선 차례에 실행한다.

## 할 일 상세 창 정리 (t-1097)

- 기존 E2E 43·46·47·52·54·67·70·75·76·92·107을 새 배치에 맞췄다. 지우기·예약 맨 앞으로는 ⋯ 메뉴(휴대폰 시트)로, 속성 칸은 「저장」 없이 파일에 쓰이는지로, 예약 확인은 맡길 글 대신 모델·저장소 요약으로 본다.
- E2E46에 「속성은 바로 저장, 고치던 제목은 남아 이어 저장해도 충돌 없음」 단계를 더했다. 저장 흐름이 두 갈래가 되며 생기는 낡은 블록 hash 충돌을 막는 회귀라 필요하다.
- E2E92 「후속 문서」 메모를 화면보다 길게 하고, 예약 확인에서 「이 내용으로 예약」이 화면 안에 있고 그 자리에서 눌리는지(elementFromPoint) 본다. 사용자가 겪은 「스크롤에 가린 예약 단추」의 회귀라 필요하다.
- 실행: `NODE_PATH=<작업 폴더 Playwright 1.64.1> node web/e2e/web.e2e.mjs <캡처 폴더> --spec 43,46,47,52,54,67,70,75,76,92,107 --workers 6`, 웹 유닛·typecheck·빌드. 공용 브라우저 캐시를 쓰고 고정 sleep·재시도·시한 연장은 없다. 전체 gate는 coordinator 차례에 한다.

## lab CLI 출력의 macOS pipe 상속 막힘 (t-1097)

- 74회차 후보 gate의 m13-4에서 `accounts::host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials`가 `30s 안에 labA 연결이 되지 않았다`로 실패했다. 같은 실험실 host trace에는 첫 `machine list`(49.923~49.979, 빈 목록) 뒤 30초 동안 front 연결이 없었다. host는 50.79에 labA를 연결했다. 시험 thread가 첫 호출의 `Command::output()`에서 돌아오지 못한 것이다(t-0787의 `ci.rs` 같은 실패와 같은 모양).
- 원인: macOS Rust std는 `pipe()` 뒤에 CLOEXEC를 따로 건다(`std/src/sys/pipe/unix.rs`, `pipe2` 없음). 그 틈에 같은 시험 프로세스의 다른 thread가 띄운 오래 사는 자식(다른 시험의 host·agent)이 쓰기 끝을 물려받으면, CLI가 끝나도 EOF가 오지 않는다. 최소 재현(한 thread가 `output()` 반복, 세 thread가 `sleep 3` 자식을 계속 띄움)에서 20초 2,622번 중 2번이 3.01초 막혔다. 출력을 파일로 받으면 2,643번 중 최대 0.15초였다.
- 수리: lab `captured()`가 표준 출력·오류를 `O_CLOEXEC`로 원자적으로 연 임시 파일로 받는다. `Node::run`(모든 `json`/`ok`)과 `command_of`가 이를 쓴다. `Node::wait_connected`는 시도마다 시각·걸린 시간·종료 코드·머신 상태·오류 첫 줄을 남긴다. accounts world가 이를 쓴다. 30초 상한과 기존 단언은 그대로다.
- 회귀 `cargo test -p aaa --test m13 lab_capture_returns_while_another_process_holds_the_output`: 출력을 든 채 남는 손자(`sleep 600 &`)로 다른 프로세스가 쓰기 끝을 든 상황을 결정적으로 만들고, 돌아온 때 그 손자가 살아 있는지(`kill -0`)로 본다. 옛 pipe 방식에서는 20초 경보로 끊겼고(RED), 수리 뒤 0.01초에 통과한다. 병렬 시험이 다른 시험의 host 수명만큼 막히는 실제 실패를 막는 회귀라 필요하다.
- 제품 host·agent에서도 `Command::output()`과 오래 사는 자식 spawn이 다른 thread에서 겹치면 같은 경합이 생길 수 있다. 이번에는 확인하지 않았다(coordinator에 별도로 알림).

## E2E93 클릭 보고와 quit의 경로 간 순서 (t-1097)

- 후보 `35ea8138` gate에서 E2E93 Claude WebKit 390 `--resume`이 「실제 클릭 보고를 받지 않았다」로 실패했다. session host 수신 순서는 누름 11바이트(51.0738) → `quit` 5바이트(51.192377, `aaa pane run`) → 뗌 11바이트(51.192869)였다. harness는 누름·quit만 받고 끝났고, 뗌 `\e[<0;24;19m`은 셸에 찍혔다. 이 spec·가짜 harness는 main과 같아 main에도 있는 시험 결함이다.
- 시험은 클릭(브라우저 WebSocket → host → agent → session)과 quit(CLI → host API → agent → session)을 서로 다른 경로로 보내고, 뗌이 닿았는지 보지 않고 quit을 보냈다. 서로 다른 입력 출처 사이의 PTY 도착 순서는 보장하지 않는다.
- 제품 확인: 마우스 보고 중 제자리 클릭은 `Term.tsx` `forwardClick`이 합성 누름·뗌을 한 처리 안에서 연달아 보낸다. 같은 spec에 마우스 사건·WebSocket 전송 기록을 잠시 넣어 돌린 실행에서 WebKit 390도 두 보고를 같은 ms(15633)에 보냈다. 따라서 실패 때 누름 뒤 약 120ms 늦은 뗌은 보기 전환 중의 뗌 처리 결함이 아니라, 부하 중 전달 경로에서 생긴 간격으로 본다. 그 간격에 다른 경로의 quit이 끼어들었다. 실패 run의 브라우저 전송 시각은 기록에 없어 직접 관측하지는 못했다.
- 수리: `quit()` 전에 harness가 입력마다 새로 쓰는 `shell-input` 기록에서 button 0의 `M`·`m` 수신을 사건으로 기다린다(기존 `until` 20초 한도). 기존 단언·시한은 바꾸지 않았다. 경합이라 수리 전 RED는 결정적으로 재현하지 않았다.
- 확인: `node web/e2e/web.e2e.mjs <캡처 폴더> --spec 93 --workers 2` 통과(2/2).

## 예약의 실제 대기 이유와 읽기 전용 조회(t-1106)

- 운영의 `account-quota-v1/<계정>`에는 `CodexRpc`의 성공한 primary 주간 창(10080분)이 있었다. 수집 자체의 부재와 5시간·주간 두 창을 강제하는 배정 정책을 구별한다. 등록 Codex는 host `quota_report` → 격리 `codex app-server` → `account/rateLimits/read` → 공통 정규화 → 로그인 세대별 SQLite 관측 경로다. ticker의 `refresh_usage`도 수집하므로 계정 화면이 없어도 갱신한다. agent의 `quota.json` 상태줄 경로는 Claude용이다.
- [공식 App Server 문서](https://learn.chatgpt.com/docs/app-server#6-rate-limits-chatgpt)와 [Codex RateLimitSnapshot](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/RateLimitSnapshot.ts)([Apache-2.0](https://github.com/openai/codex/blob/main/LICENSE))은 primary·secondary를 각각 nullable로 정의한다. [BullMQ QueueGetters](https://github.com/taskforcesh/bullmq/blob/master/src/classes/queue-getters.ts)([MIT](https://github.com/taskforcesh/bullmq/blob/master/LICENSE))의 상태 조회와 변경 분리를 참고했다. 외부 코드·의존성은 가져오지 않는다.
- `account_wait_does_not_hide_the_next_reservations_actual_reason`은 실제 Host·SQLite·가짜 agent 조회로 병합 대기·착지·보류·계정 풀의 만료 인증을 함께 만든다. 앞 예약의 계정 대기 뒤 예약도 자기 머신 조건을 판정해야 한다. 필요 이유: 모든 `start_wait`를 자리 부족으로 취급하여 뒤 예약이 「자리 대기」로 가려진 회귀를 막는다. t-1107의 사용량 판단 불가 실행 정책은 그대로 두고 확실한 인증 오류를 fixture로 쓴다. 실제 자리 부족의 차례 보존은 기존 `slot_released_during_queue_scan_does_not_bypass_the_moved_front`가 계속 지킨다.
- `queue_query_preserves_approval_order_reason_and_history`는 Reader의 `QueueOp::List` 반복 조회 전후 승인판·차례·원인·시각·TASKS.md·변경 이력이 같음을 확인한다. 필요 이유: 조회를 위해 reserve를 다시 호출하여 기존 원인과 승인판을 덮는 운영 실수를 예방하고 읽기 계약을 지킨다. CLI는 기존 `aaa task queue --json`을 쓴다.
- 기존 `fake_rpc_reads_without_threads_turns_or_raw_credentials_and_cleans_each_home`에 primary가 주간이고 secondary가 null인 응답을 추가한다. 필요 이유: 단일 주간 관측이 수집 성공으로 보존되고 없는 5시간 창을 합성하지 않음을 실제 helper 교환으로 확인한다. 인증 canary 제외·무작업 RPC·임시 홈 정리 단언을 유지한다. 새 debug 진단은 원문의 창 개수와 정규화된 기간만 기록해 제공자 부재와 파싱 탈락을 구별하며 인증·원문은 기록하지 않는다.
- 배정 정책은 별도 t-1107 범위다. 이 시험은 해당 선택 함수를 바꾸지 않는다. 관련 검사만 자기 worktree에서 실행하고, 전체 gate·병합·push는 coordinator의 묶음 착지 차례에 맡긴다.


## 재연결 중 완전 thread 목록과 전송 ACK 경계(t-1088, 74회차 수리)

- E12의 `initial-list` 실패 원문은 같은 문서의 연결1/2/3에서 첫 요청이 시작됐고, 먼저 시작한 읽기는26페이지/2,505건을 완료했지만 최신 읽기는11페이지만 받은 상태였다. 기존 `ThreadReload.finish`가 최신 시작만 허용해 완전 목록을 버렸다. 새 연결의 첫 실제 HTTP 응답을 보류하고 옛 연결의 마지막 응답만 전달하는 결정적 대조에서도 수정 전 제품은20초 안에 마지막 행을 표시하지 못했다. main a832f97a의 같은 클래스도 순수 사건 순서 재현에서 완료 목록을 버린다. 페이지화가 이 기존 경합의 노출 시간을 늘린 제품 결함이다.
- 진행 중인 각 읽기에 수신 사건을 보관한다. 먼저 끝난 완전 목록을 반영하고 더 새 읽기가 끝나면 이전 읽기는 무효화한다. 새 읽기 시작/실패만으로 이미 완료 가능한 목록을 가리지 않는다. 부분 목록을 표시하거나 페이지 개수/크기/커서/HTTP 계약을 바꾸지 않는다. 기존 `threadReload.test.mjs`에 두 회귀를 더해 미완료 새 읽기·모든 읽기의 사건 보존·새 완료 뒤 늦은 옛 응답 무시와 새 읽기 실패 뒤 옛 완료 반영을 검사한다. 필요한 이유는 재연결 반복으로 초기 목록이 영원히 비거나 새 보고가 옛 응답으로 돌아가는 회귀를 함께 막기 위해서다.
- 정규 E12의 두 엔진에서 새 연결의 첫 응답을 보류한 채 옛 전체 목록의 실제 마지막 행을 확인한 다음 새 읽기를 푼다. 두 독립 완료 읽기의 정확한2,505개/차례/고유값과 DOM2,505개 단언은 유지한다. `route.fulfill` 완료는 브라우저의 `response.body()` 수집 완료와 다르므로, 후자는 실제 본문 수집 사건에서 두 완료 목록을 기다린다. 첫 수리 확인에서 최종 response 사건이 수집 단언보다13ms 늦어25/26페이지로 잡힌 원문을 보존하고 이 관측 경계도 바로잡았다. 기존20초 상한·최초 오류/정리 수명 계약은 유지한다.
- E23은 두 번째 `submitted` ACK와 후속 `pane_chat` 응답만 보류해 원래 gate의 pending2/단추1/옛label/앞말 위치를 결정적으로 만들었다. 두 번째 말의 stage는 `sending`, 첫 말은 `sent`였고 입력창은 다음 초안 편집을 위해 이미 readonly=false였다. ACK를 전달하자 `sent/sent`로 바뀐 같은 DOM commit에서 단추가 마지막 말·`2개 지금 보내기`로 함께 바뀌었다. main a832f97a와 통합 d0e6217b 모두 같아 제품 부분 렌더가 아닌 시험의 중간 상태 단언이다.
- E23의 기존 최종 deepEqual을 그대로 두고 같은20초 사건 대기에서 pending2·단추1·마지막 위치·정확한label·idle을 함께 기다린다. 낙관적 말풍선 수/편집 가능을 전달 증거로 쓰지 않는다. 진단용 ACK 보류/MutationObserver의 원문은 보고 library의 증거용 텍스트로 보존하며 정규 시험에 제품 상태 조작이나 새 의존성을 넣지 않는다. 기존 Claude/Codex/휴대폰·키 바이트·거절/흡수·진단 계약과 E2E 번호를 유지한다.
- [TanStack Query cancellation](https://tanstack.com/query/latest/docs/framework/react/guides/query-cancellation)([MIT](https://github.com/TanStack/query/blob/main/LICENSE))의 취소/기존 data 보존, [React snapshot](https://react.dev/learn/state-as-a-snapshot)([MIT](https://github.com/facebook/react/blob/main/LICENSE))의 한 render 상태, [Playwright response.body](https://playwright.dev/docs/api/class-response#response-body)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))의 실제 응답 완료를 비교했다. 기존 전체 목록/사건 모델과 단추 전달 단계 계약을 유지하는 수리를 택했고 외부 코드·의존성은 추가하지 않는다.
## 완료 보고의 후속 원장(t-1112, §8.503)

- host 회귀 `completed_next_survives_merge_inbox_cleanup_and_task_mutations_until_linked`는 실제 SQLite·TASKS·완료 보관·inbox를 써 병합된 결과의 자동 보관 차단, 삭제/완료 체크/원문 연결 해제 거부, inbox 개별·일괄 정리 보존, 자기 연결·없는 ID·빈 사유·처리 덮어쓰기 거부, thread 내부 actor 거부와 두 writer의 barrier 동시 처리 한 번 확정, 실재 후속 연결 뒤 자동 보관을 검사한다. 필요한 이유: 이번 유실 경로와 종료 우회 경로를 같은 원장으로 막아야 한다.
- `no_linked_task_and_replaced_reports_keep_pending_with_durable_cleanup_reason`은 연결 task 부재·새 보고의 Next 삭제·같은 사본 재회수·명시적 정리 사유·Host 재기동을 확인한다. 필요한 이유: 파일이나 임시 알림만 남기면 보고 수정/재시작에서 후속을 잃는다.
- 기존 E2E47은 짧은 thread 주소의 hashchange를 capture listener로 보류하고 초기 폼 입력 뒤 전달해 실제 노드/입력이 유지되는지 확인한다. 첫 브라우저 시험이 주소 사건에서 화면 전체를 내렸다 다시 만들어 입력을 비우는 결함을 찾아 App에서 바로 같은 thread로 해석하도록 고쳤다.
- 기존 E2E47에 실제 host·agent·CLI·웹의 완료→resolve→후속 경고→실재 후속 생성→없는 ID 거부→웹 연결→보관/복원 흐름을 덧붙인다. Chromium/WebKit × 1280/390px에서는 정리 사유와 영수증·새로고침·동시 읽기 기기 갱신·Reader 직접 요청 거부·가로 넘침을 검사한다. 필요한 이유: 실제 권한/사건 갱신과 두 웹 엔진의 폼은 host 유닛으로 대체할 수 없다. 공용 fake 보고의 Next를 쓰던 기존 m13 완료 보관 owner들은 그 Next 확인을 명시적으로 기록한 뒤 원래 보관 계약을 검사한다.
- 실행: `cargo test --profile verify --locked -p aaa-host --lib`, `cargo test --profile verify --locked -p aaa --test m13 <owner>`, 기존 E2E47. Playwright 1.64.1 모듈은 자기 작업 폴더에 두고 브라우저는 내부 공용 캐시와 launchChromium을 쓴다. 고정 sleep·실패 재시도·시한 확대는 없다. 전체 ci-local은 묶음 착지 차례에 coordinator가 수행한다.

### 75회차 착지 회귀 수리(t-1112)

- 첫 전체 gate에서 m13 여섯 회귀를 확인했다. 후속 출력을 CLI 머리 앞에 넣어 worktree/reopen의 기존 첫 줄 계약을 깬 두 건은 후속을 머리 뒤로 옮겨 고친다. 완료 보고가 왔지만 열린 thread에 새 inbox를 미리 만들어 새 보고/nudge 묶음을 바꾼 두 건은 원장 기록과 종료 알림을 분리해 고친다. 다시 열기는 이미 생긴 후속 알림을 지우지 않는다. 기존 상한·문자열·개수 기대는 유지한다.
- main 조상/묶음 전이 owner 두 건은 공통 fake가 먼저 남긴 완료 Next가 안전망에 걸렸다. 기존 helper로 그 결과 확인을 명시적으로 기록하고, 원래 Git 판정/묶음 전이/보관 단언을 유지한다. 새 host 회귀 안에서 열린 보고→resolve→다시 열기의 알림 수명도 검증한다. 실패 원문과 30초 load 기록은 착지 보고에 보존한다.
- 같은 gate의 E2E67·77도 보관 fixture의 결과 확인/착지 대기 Next를 남긴 채 보관을 기대했다. 두 owner 안에서 실제 완료 회수 뒤 명시적 사유를 기록한 다음 원래 행 제거/복원/ID 단언을 유지한다. 공통 harness의 보고나 제품 판정·시한은 바꾸지 않는다.

- 수리본 전체 gate는 Rust 전체/나머지 단계가 통과하고 E2E12 하나가 실패했다. 원문 trace의 최초 오류는 initial-list의 20초 미완료이며, 이후 unroute 정리에서 발생한 늦은 fulfill 거절이 Node unhandled rejection으로 최초 오류와 실험실 정리를 가렸다. route handler의 오류를 해당 단계/정리 기록에 보존하고 기존 단계 오류를 먼저 돌려준다. 예상하지 않은 실행 중 route 오류도 성공으로 넘기지 않는다.
- E2E12의 보고·Next 없는 2500개 복제 기록이 첫 thread의 후속 원장을 상속한 것을 확인했다. 독립 기록의 후속/판 원장을 비우고, 원래 대량 stdout>1MiB·2,505개·페이지 크기·차례·고유성·두 완전 읽기·DOM·20초 대기는 그대로 유지한다. [Playwright unrouteAll](https://playwright.dev/docs/api/class-browsercontext#browser-context-unroute-all)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))의 실행 중 handler 대기와 오류 수명을 참고했고 외부 코드는 복사하지 않았다.

## Windows 목록 정리와 macOS pipe 창 신호(t-1123)

- main `df399e30`의 목록 회귀 두 개는 Windows CI와 격리 실기에서 모두 `Lab::drop`의 공유 위반(OS 오류 32)로 실패했다. t-1038의 sweep과 같은 삭제 순서 문제다. 이번 fixture에는 별도 agent/task/DB worker가 없으므로 `host` 뒤에 디렉터리 guard를 선언해 동기 rusqlite 연결을 먼저 해제한다. 성공·unwind 모두 같은 순서이며 삭제 실패에는 경로·원래 OS 오류를 남긴다. 삭제 재시도·sleep·오류 무시는 없다.
- 대량 시험은 2,507개 원장과 모든 페이지·역할·상태·본문·frame·cursor 단언을 그대로 둔다. Windows에서 정리 수리 후에도 두 시험 합계 7.50초가 걸려 준비 데이터의 2,507번 개별 commit만 한 transaction으로 묶는다. 제품의 목록·저장 코드는 바꾸지 않는다.
- pipe 창 회귀의 기존 stdout 준비 신호는 libtest의 `test … ... ` 접두사가 같은 줄 앞에 붙으면 부모의 `starts_with("INHERITED=")`에서 버려졌다. 단일 시험 스레드로 실제 holder를 실행한 대조에서 raw holder는 상속 확인·입력 대기까지 도달했으나 부모는 준비 신호 읽기에서 실패했다. 준비/해제 신호는 전달된 stdin의 양방향 UnixStream으로만 주고받고 stdout은 libtest에 맡긴다. holder에 `--test-threads=1`을 명시해 접두사가 생기는 경계를 정규 회귀에 유지한다. 원래 CI에는 자식 단계 기록이 없어 그 실행의 같은 원인 여부는 소급 확정하지 않는다.
- raw/std/Tokio/실제 harness 판 조회 네 경로의 실제 pipe 창·CLOEXEC·상속 유무·capture exit 7·stdout/stderr 보존·holder 생존 중 출력 완료 계약은 유지한다. 모드·역할·PID·pipe 진입/해제·holder 생성·capture 연결/NOTE_EXIT·출력 수집을 별도 파일에 기록한다. 부모는 프로세스 종료와 stdout/stderr EOF를 각각 관측해 바깥 출력 대기가 실패해도 마지막 단계를 보고한다. argv·환경·인증은 기록하지 않는다. 내부 실패 상한은 2초, worker 상한은 5초로 줄여 기존 내부/외부 30초 동시 만료가 진단을 가리는 문제도 없앤다. 정상 실행은 사건으로 즉시 끝난다.
- 필수 부분 검사는 macOS `cargo test --profile verify --locked -p aaa-common --lib child_process::tests::pipe_window -- --nocapture`의 단독/단일 스레드/일반 병렬 비교, 두 OS의 `cargo test --profile verify --locked -p aaa-host --lib orch::list_tests`, strict clippy·fmt·공개 검사다. 새 시험·E2E 번호·제품 변경은 없으며 기존 세 회귀를 강화한다. 전체 gate는 coordinator 차례에서 수행한다.
- OSS 후보는 [tempfile의 handle 해제와 명시 close](https://github.com/Stebalien/tempfile/blob/master/src/dir/mod.rs)([MIT/Apache-2.0](https://github.com/Stebalien/tempfile/blob/master/LICENSE-MIT)), [Tokio의 자식 wait와 출력 수집](https://github.com/tokio-rs/tokio/blob/master/tokio/src/process/mod.rs)([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)), [Rust std의 pipe 생성](https://github.com/rust-lang/rust/blob/main/library/std/src/sys/process/unix/unix.rs)([MIT/Apache-2.0](https://github.com/rust-lang/rust/blob/main/LICENSE-MIT))이다. 자원 해제 순서와 제어/출력 분리 원칙만 참고하며 외부 코드·의존성은 추가하지 않는다.
- 최종 부분 확인: Windows 두 목록 시험 합계 1.00초(수명 수리만 한 판 7.50초), macOS 단일 스레드 pipe 회귀 0.23초, 일반 병렬 aaa-common 57개 통과·기존 ignored 1개/0.94초와 목록 두 개/0.93초다. strict aaa-common/aaa-host all-targets clippy·fmt·공개 검사도 통과했다. Windows 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준 목록이 전후 같고 격리 시험 폴더를 삭제했다. 제품 배포는 필요 없다.

## E2E12 본문 관측과 실패 자식의 소유권(t-1123, 76회차)

- 78회차의 추가 경계: 다운로드 완료(`requestfinished`) 뒤에도 Node의 `response.body()` protocol 호출이 문서 교체보다 늦게 도착할 수 있다. 실제 Chrome에서 본문 호출만 사건 gate로 보류→문서 교체→gate 해제로 `No resource with given identifier found`를 재현한다. 읽는 중인 요청도 같은 page의 실제 timeOrigin 교체를 기록하며, 이때 잃은 본문만 취소 꼬리로 구분한다. 본문을 받은 뒤의 JSON/내용 오류와 현재 문서의 body 오류는 계속 실패한다. 기존 수명 회귀에 이 경계와 현재 문서 오류 대조를 더했으며 새 단계·시한·번호는 없다. 필요한 이유: 헤더만 온 요청과 다운로드까지 끝난 요청은 서로 다른 protocol 수명을 갖는다. 수리 후 전체 E2E12 50.5초, 새 경계 포함 수명 회귀0.120초였다.
- 앞선 `5d088c34`는 route handler 해제→context 닫기, `df399e30`은 두 완전 목록의 실제 body 수집과 재연결 제품 상태, `e3bcc847`은 복제한 후속 원장 제거와 route 오류 회수를 고쳤다. 이번도 같은 비동기 수명 경계 계열이지만, 완료 전 HTTP 본문 Promise와 단계에서 분리된 WebKit 생성의 소유권은 그 수리에 포함되지 않았다. 세 수리의 기존 단언은 유지한다.
- 첫 76회차 gate는 Chromium 초기 문서의 요청32가 헤더 뒤 미완료였고 reload 뒤에도 `response.body()`가 남아 `reads()`의 `Promise.all`이 끝나지 않았다. 기존120초 단계 제한이 browser.close를 시작한 뒤에야 body 오류가 나왔다. 작은 실제 HTTP 서버에서 헤더/부분 본문→문서 교체→서버 본문 종료까지도 기존 body Promise가 pending이고 browser.close 뒤에만 거절되는 것을 사건으로 재현했다. route.fetch·fulfill 완료만 기다리는 것으로는 이 브라우저 protocol 수명을 끝낼 수 없다.
- 이제 `response`에는 객체만 보관하고 `requestfinished` 뒤 본문을 수집한다. 초기 문서의 이미 완료한 body 작업은 reload 전 회수한다. 실제 main frame의 timeOrigin 교체로 중단된 꼬리는 `navigation-aborted`/`cancelled-tail`로 기록하고, 앞 커서·중복·정확한 목록 접두사를 검사한다. 취소 꼬리는 완전 페이지로 세지 않는다. 정상 응답은 여전히 상태200·frame/페이지 상한·ID 순서·고유값·전체 개수와 두 완전 reload 읽기를 모두 검사하며 현재 문서의 네트워크/본문 오류는 실패다. 알려진 미완료 요청을 성공 페이지로 바꾸거나 버리지 않는다.
- 단계의 Promise.race는 작업을 취소하지 않는다. 원래 큰 단계에서 WebKit을 먼저 띄운 뒤 Chromium 관측이 제한에 걸리면 그 함수의 finally가 실행되지 않아 WebKit이 실험실 밖에 남았다. `cleanupScope.own`이 비동기 생성 시작 전부터 release를 등록하고 runSpec이 root browser 닫기 전 소유 자원을 모두 회수한다. 정상 경로의 지역 close도 같은 한 번의 정리를 공유한다. 실제 실패 child에서 등록 없는 대조는 owner 종료 시 WebKit exitCode=null, 수리판은 exitCode=0이었고 원래 실패 exit1을 보존해 자연 종료했다. 타 작업이나 pattern PID 종료는 없다.
- 기존 E2E12 안의 두 작은 회귀는 실제 헤더/문서 교체를 browser.close 없이 회수하고, 실제 host/agent·WebKit 자식을 만든 미완료 작업이 실패해도 자식 종료와 원래 오류/exit1을 지키는지 본다. 필요한 이유는 각각 이번 body 대기와 CI 프로세스 미종료를 결정적으로 막는 것이다. 고정 sleep·실패 재시도·시한 확대는 없다. route 오류 진단에는 Playwright Call log의 쿠키/헤더를 빼고 오류 첫 줄·stack frame만 남긴다.
- 5초 원칙을 적용해 같은 2,500건 fixture를 재사용하는 CLI 준비/거르기, Chromium IPC/HTTP/DOM, WebKit IPC/HTTP/DOM의 세 고유 계약으로 나눴다. 단독 실행은 각각2.584/2.971/3.737초, 새 수명 회귀는0.044/1.817초였다. E2E12 전체 파일46.5초와 개별 계약 시간을 구분한다. 기존 「이 기기 알림」 계약은 같은 단독에서15.134초였으며 전체5초 감사(t-1117)의 별도 대상이다. 숫자를 맞추기 위한 기대값/시한 변경이나 검사 삭제는 없다.
- 참고: [Playwright 네트워크 사건 순서](https://playwright.dev/docs/network#network-events), [Response.body](https://playwright.dev/docs/api/class-response#response-body), [BrowserContext.unrouteAll](https://playwright.dev/docs/api/class-browsercontext#browser-context-unroute-all)([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와 Node의 [자식 프로세스 exit 사건](https://nodejs.org/api/child_process.html#event-exit)([MIT](https://github.com/nodejs/node/blob/main/LICENSE))을 대조했다. 외부 코드는 가져오지 않았다.
- 수리판의 관련12파일/23작업, worker10 병렬은71.6초에 모두 통과했다. E2E12의 새 두 수명 회귀0.048/2.111초, CLI/Chromium/WebKit4.225/3.806/4.218초로 평소 병렬에서도 각5초 미만이다. 웹 유닛606개도 통과했다. 이 확인 뒤 새 전체 gate를 수행한다.
- ci-watch가 main `e3bcc847`에서 분류한 Windows 목록 두 건·macOS pipe 창 재발은 수리 전의 같은 owner다. 별도15분 macOS test action 상한은 전체 job의 시간 사건이며, 이번30초 pipe 대기 수리만으로15분 초과 전체가 설명된다는 근거는 없다. 착지 후 CI 회복은 host 감시가 별도로 확인하며 push를 기다리게 하지 않는다.

## 질문 원문 글자·형식 진단·답한 질문 재보고(t-1124, §8.504)

- proto `reordered_choices_keep_source_letters_and_ambiguous_letters_are_diagnosed`는 B/A/C의 글자·추천을 그대로 읽고 중복·누락을 진단하며 구판 자료의 위치 fallback을 지킨다. host `reordered_report_composes_the_source_letter_instead_of_the_position`는 실제 전달문이 배열 0에서 `1B`가 되는지 검사한다. 꼭 필요한 이유는 q-1408~1412·q-1434의 본문/카드/답 글자 불일치 회귀를 막기 위해서다.
- 기존 host `question_format_warning_allows_answers_to_reach_delivery_and_notifies_once`는 읽힌 선택지의 원문 글자와 직접 답, 보고 복사 성공, 작업 inbox·pane 전달 진단 및 같은 판 경고 1회를 확인한다. 자동 답의 `automatic_reply_keeps_the_source_letter_on_reordered_choices`는 B/A 원문의 B가 위치 0에 기록되는지 검사한다. 구판 CLI wire 회귀 `legacy_cli_answers_require_source_letters_only_for_noncanonical_cards`는 B/A 카드의 구판 숫자 답은 갱신 오류이고 일반 A/B는 기존 전송 경로까지 가는지 실제 duplex로 확인한다. 꼭 필요한 이유는 구판 CLI가 다른 선택지를 조용히 답하는 것을 막기 위해서다.
- `answered_reports_reuse_ids_and_mixed_reports_open_only_new_questions`는 실제 SQLite 답 기록→보고 회수→질문 목록→재시작 재해석을 잇는다. 같은 질문은 원래 ID/답·답 기록 한 건을 유지하고, 새 질문이 섞이면 원문 2번만 열어 `2B`로 전달한다. `repeated_question_requires_committed_answer_same_thread_and_unchanged_material`는 임시 claim·다른 thread·질문 내용 변경·같은 URL의 그림 변경/누락을 흡수하지 않는다. 꼭 필요한 이유는 q-1440 뒤 q-1441처럼 방금 답한 질문의 재생성과 새 판단의 잘못된 종료를 함께 막기 위해서다.
- 기존 E2E53의 `question-letters.mjs`는 Chromium 1280px·WebKit 390px에서 실제 보고의 B/A/C 순서·본문 표·웹 B 답→PTY 원문 글자·그때 답 카드, 보고 밖 글만 바꾼 재보고의 동일 q-ID/답 기록 수, thread로 수정 안내 전달, 혼합 보고의 2번만 표시·CLI `2A`의 원문 A, q-ID CLI B, 중복 글자 오류 카드·thread 직접 알림을 확인한다. 기존 오류 회귀의 A 재명명 기대는 없애고 실제 B 글자 보존을 검사하며, 다음 별개 CLI 질문은 제목을 달리해 중복 방지 계약과 구별한다. E2E69·74로 coordinator 질문·기본 검수 카드 호환을 확인한다. 새 E2E 번호는 없다.
- 각 회귀는 사건·응답·DOM·PTY 출력으로 끝난다. 고정 sleep·재시도·시한 확대·기대값 완화는 없다. Playwright 1.64.1 npm만 작업 폴더에 두고 공용 내부 브라우저 캐시와 저장소 launchChromium helper를 쓴다. 부분 확인은 proto 전체·host 유닛·CLI/의존 경계·웹 타입/606개 유닛·E2E53/69/74·strict clippy·공개 검사이며 전체 gate는 coordinator 묶음 착지 차례에 맡긴다.
- OSS 후보·라이선스와 채택한 원칙은 plan §8.504에 있다. 외부 코드·의존성을 추가하지 않는다.

- 첫 host 전체 유닛은 직접 알림의 큰 비동기 호출을 collect 경로에 인라인으로 넣어 기존 idle-audit 시험의 스택이 넘쳤다. 알림의 thread_message future를 Box::pin으로 두어 호출자 크기를 줄인 뒤 해당 회귀와 host 전체 695개가 통과했다. 새 E2E의 첫 실패는 HTTP의 ok 포장을 빠뜨린 시험 코드였고, 다음 개수 대조 실패(5→6)는 답 전송 전 claim을 완료로 읽은 관측 경계였다. 보존 DB에는 해당 q-ID 답이 정확히 한 건이었다. 정규 검사는 실제 asks 원문 기록이 생긴 사건을 기다린 뒤 재보고 전후 개수가 같음을 단언한다. 시한·개수 기대·재시도 정책은 바꾸지 않는다.

### 77회차 기본값 시험의 완료 후속 경계(t-1126)

- 첫 전체 gate의 `start_defaults_profile_kind_and_task`는 공통 fake의 완료 보고·Next를 먼저 회수한 경우 기존 t-1112 후속 보호에 막혔다. 원문은 `t-0005를 task에 잇지 못했다(t-0002 후속 남음)`이며 실패 실험실에 보존된 완료 보고의 Next 두 줄과 TASKS 연결을 확인했다. 후보가 바꾸지 않은 기존 main의 fixture/가드 경합이다.
- 해당 owner에서 완료 보고 회수를 사건으로 기다리고 기존 `finish_fixture_followups`로 가짜 결과 확인·다음 단계 검토 사실을 기록한 다음 task 연결을 바꾼다. 제품 보호·공통 fake·성공과 정확한 경고 문구·시한은 그대로다. 새 시험이나 sleep·실패 재시도는 추가하지 않는다. 필요한 이유는 완료 보고 수집 속도가 기본 profile/kind와 열린 task 시작 경고 시험의 성공 여부를 결정하지 않게 하기 위해서다.

- 두 번째 전체 gate는 Rust 전체가 통과하고 E2E53의 수정 M3 카드에서 `thread-ask-pick-2B`를 기다리다 실패했다. 보존 DB에는 broken M3의 두 질문 직접 답이 이미 asks에 기록되고 수정 보고에는 원문1번만 남아 있었다. 답한2번을 새 중복 방지가 정상 제외한 것이다. 기존 owner는 전달 전 claim인 `questions.answer`만 보고 다음 보고를 써 실제 원문 보존 순서에 따라 결과가 갈렸다.
- E2E53의 해당 owner에서 이전 답의 시각·질문ID가 실제 asks 원문에 보존된 사건 뒤 다음 보고를 쓴다. 같은 원문을 복구하면 답한2번이 제외됨을 먼저 확인하고, 다시 판단할2번은 질문 제목으로 새 판단임을 명시해 원래 두 질문·3/2개 선택지·정확한 `1C 2B` 전달 단언을 유지한다. 원본 M3 fixture·제품 중복 방지·시한은 바꾸지 않는다. 필요한 이유는 전송 전 claim과 실제 답 보존을 구별하며 새 질문과 답한 질문의 기존 두 계약을 모두 지키기 위해서다. 해당 파일 전체의 부분 검증은82.1초에 통과했고 최종 추가 단언은 전체 gate에서 확인한다.

## 인증 fixture의 공통 opaque 토큰(t-1127)

- 80회차 통합에서 새 main의 `large_report_and_bad_parts_keep_requests_on_one_link`에 남은 직접 인증 fixture를 공통 `CANARY`로 옮긴다. 생성기를 통과한96바이트 토큰도 기존 길이 기준 계산으로 조각 경계를 가로지르며 원본 hash 보존·재조립 뒤 가림·다음 요청 단언과5초 상한을 유지한다.

- [시험 인증 fixture](test-secrets.md)의 공통 생성기·벡터를 Rust·Python·Node E2E가 함께 쓴다. 계정/업무 이름은 seed나 별도 대응표에만 두고 본문에는 넣지 않는다. 제품 가림 규칙은 바꾸지 않는다.
- `scripts.test_fixture_secrets`와 웹 `fixture-secret.test.mjs`, `aaa-test-support` 유닛은 언어별 공통 벡터·업무 단어 즉시 거부를 검사한다. 정적 검사는 ci-local 시험 진입 전에 실행하고 정확한 경로·값·사유 예외만 허용한다. 필요한 이유는 같은 실행의 다른 owner까지 망가뜨리는 평문 인증 fixture 재유입을 막기 위해서다.
- 기존 aaa-common 가림 owner에 `opaque_credentials_do_not_redact_business_fields_in_the_same_process`를 더한다. 실제 전역 가림 등록 뒤 토큰은 가려지고 automation/owner/인증 JSON 필드는 보존되어야 한다. 필요한 이유는 생성기 형식 검사만으로 실제 제품 serializer의 필드 보존을 대신할 수 없기 때문이다.
- 기존 계정·금고·이름 변경·자동화·사용량·세션·hook·설치 암호·K0·이미지의 인증 fixture를 옮기고, 바이너리 base64 충돌과 모든 8바이트 부분 누출·변환·권한/오류 계약을 유지한다. E2E58/65/68/78/81과 M4 보호 암호 입력을 사용하며 E2E 번호는 늘리지 않는다. JWT metadata와 로그인 파서의 필수 접두사·escape·ANSI만 해당 owner에서 유지한다.
- Windows 두 실기 스크립트는 공통 const 산출물을 읽는다. 격리 폴더에서 두 전체 스크립트의 PowerShell 구문과 실제 인증 JSON의 세 필드를 검증한다. 서비스·예약 작업·Run·사용자 PATH·방화벽 목록의 전후 동일과 시험 폴더 삭제를 확인하며 설치·서비스 시작은 실행하지 않는다.
- `test_auth_fixture_guard_fails_before_python_and_rust_execution`은 격리한 실제 ci-local에 업무 단어 토큰을 넣어 Python·Rust owner 실행 전 실패와 원문 없는 경로·줄 진단을 확인한다. 필요한 이유는 검사 함수를 호출하는 유닛만으로 CI 진입 연결을 대신할 수 없기 때문이다.
- 고정 sleep·재시도·시한 확대·기대값 완화는 없다. 부분 검증 뒤 전체 gate와 묶음 착지는 coordinator의 순서를 기다린다. OSS 후보·라이선스·채택한 원칙은 위 문서에 있다.

## 보고 사본 핵심 근거 우선 선택(t-1132, §8.506)

- `nice -n 15 cargo test --profile verify --locked -p aaa-agent --lib report_copy`의 `real_size_budget_preserves_report_links_and_small_evidence_before_images`는 실제 2,399개 sparse 파일(metadata 크기: 중간 PNG 2,390×224KiB·요약 6×1KiB·연결 JSON 2KiB·그림 2MiB·영상 201MiB)에서 200MiB 상한과 근거 보존·선택 순서·누락 크기/이유를 확인한다. 필요한 이유: 이름순 그림이 작은 근거를 밀어내던 실제 규모 회귀를 막는다. 큰 본문을 읽지 않는 제품 선택 함수를 직접 호출해 불필요한 수백 MiB 해시 없이 검증한다.
- `collect_keeps_priority_hashes_and_skips_without_following_symlinks`는 작은 실제 collect로 Markdown·참조 정의·percent 주소·공백·HTML·본문 경로의 선택 순서와 원문 hash/파일 SHA-256·상한 0 probe·symlink 거부를 확인한다. 필요한 이유: 선택 단위 시험이 대신할 수 없는 실제 전송 manifest와 원문 보존을 지킨다. 새 회귀 두 개는 합계 0.46초로 확인했다.
- 기존 E2E12의 실제 부분 사본 thread에서 배너·누락 경로·파일 행 표시를 확인한다. 기존 E2E73은 실제 thread의 HTML/Markdown 보존 판에 누락이 남는지 Chromium/WebKit ×1280/390px에서 각각 확인한다. 필요한 이유: HTML iframe을 보거나 원래 thread가 정리된 뒤 r 판을 읽을 때도 누락이 숨지 않아야 한다. 응답·DOM 사건으로 끝내며 고정 sleep·재시도·시한 확대는 없다.
- 전체 gate는 coordinator에게 준비됨을 알리고 받은 차례에 실행한다. 후보를 push하지 않고 병합 준비 보고를 남기며 운영 DB·설정과 원래 worktree는 건드리지 않는다.

- 첫 전체 gate(4dc867f5, 418초)는 E2E42 하나만 실패했다. 같은 실행 파일·실제 thread에서 경고 띠만 숨긴 대조는 diff 본문 높이360.5→433.5px·파일 머리4→6개를 보였다. 새 경고가 가상화 경계 밖으로 두 머리를 밀어낸 원인이다. thread 안내를 `.thread-col` 스크롤 안으로 옮겨 옆 diff 높이를 보존한다. E2E42의 원래 6개·선택 파일·20초 단언을 그대로 검증하며 진단용 DOM 숨김은 제품·정규 시험에 넣지 않는다. 수리 후 전체 gate는 새 차례를 받아 실행한다.

## 자동화 시작 thread의 Next 제외(t-1133, §8.507)

- `followups::tests::automation_origin_skips_next_but_same_named_human_thread_keeps_protection`은 실제 SQLite·보고·TASKS·inbox·Host 재열기로 자동화의 Next 미수집과, 같은 이름/감시 문구인 일반 thread의 원장·보관 차단 유지를 검사한다. 필요한 이유: 문구 추측이나 자동화 설정 조회로 사람의 후속을 없애는 회귀를 막는다.
- `followups::tests::legacy_automation_next_handoff_is_durable_and_preserves_explicit_receipts`은 이미 판정한 구판 미처리 항목을 정상 정리 순회로 인계하고 원본 ID/판/줄·자동화 이름/host/시각·기존 명시 영수증·inbox 종료·보관·재기동/재순회 불변을 검사한다. 필요한 이유: 새 보고가 없는 종료 감시 thread의 후속 경고도 수동 사유 입력 없이 해소하고 이전 처리 근거를 보존해야 한다.
- 기존 m13 `automation_thread_reports_do_not_wake_their_own_rule`에 실제 do.start 출처의 완료 Next 표시 유지·후속 미수집·일반 시작의 수집 대조를 추가한다. 새 프로세스 실험실·E2E 번호를 늘리지 않는다. 기존 자동화 보고 사건의 순서/자기 재실행 방지 단언은 유지한다.
- 부분 확인은 `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib followups::tests`와 `nice -n 15 cargo test --profile verify --locked -p aaa --test m13 automation_thread_reports_do_not_wake_their_own_rule`다. 개별 새 시험은 보통 부하에서 5초 이하로 유지하고 고정 sleep·재시도·시한 확대·기대값 완화는 없다. 전체 gate는 coordinator가 정한 같은 머신의 실행 차례를 받아 실행한다.
## Ubuntu 메뉴 시험의 PTY slave 상속(t-1150)

- 원래 두 Ubuntu 실패 로그에는 `Working → ESC-DECODE-GATE → pick:<cancel>`까지 있다.
  입력 정지가 아니라 그 뒤 EOF 대기 실패다. 원본 순간의 상속 PID/FD는 수집되지 않았다.
  Linux 격리 진단에서 원래 메뉴의 안내·`stty -icanon` 완료·전송 `1b 0a`·dd 수신 `1b 0a`·
  취소/exit0·EOF를 나눴다. 별도 자식이 slave를 상속하면 exit0 뒤에도 EOF가 없고,
  그 자식의 해제 뒤 EOF가 왔다. 상속하지 않으면 같은 자식이 살아 있을 때 EOF가 왔다.
- portable-pty 0.9.0은 Linux에서도 `openpty` 반환 뒤 별도 `fcntl`로 CLOEXEC를 건다.
  공통 생성 잠금을 Unix에 적용하고 lab의 PTY 열기·자식 생성 및 같은 m13 프로세스의
  직접 std Command 호출을 같은 잠금에 넣는다. wait·출력 수집은 잠금 밖이다.
  메뉴 화면·Working/취소 판정·출력 EOF 수집과 기존 대기 상한은 유지한다.
  PTY 실패 진단에는 자식 PID/종료 상태와 안내별 전송 바이트 수·정리 결과를 남긴다.
- Linux GNU의 `child_process::pty_tests::pty_open_window_does_not_leak_slave_or_delay_eof_until_another_child_exits`
  는 LD_PRELOAD를 자기 worker에만 주고 실제 openpty 반환 직후 창을 사건으로 붙잡는다.
  raw 대조의 실제 slave 상속·exit7 뒤 EOF 부재·holder 종료 뒤 EOF와,
  std/Tokio의 상속 없음·holder 생존 중 EOF·전체 출력/exit7 보존을 검사한다.
  필요한 이유: Linux pipe2만으로 PTY도 안전하다고 가정한 회귀를 실제 FD 경계에서 막는다.
  기존 macOS pipe 창 회귀도 유지한다. 외부 코드·제품 hook·새 E2E 번호는 추가하지 않는다.
- 부분 검사는 `nice -n 15 cargo test --profile verify --locked -p aaa-common --lib`,
  `nice -n 15 cargo test --profile verify --locked -p aaa --test m13`와 두 crate의 strict clippy다.
  Linux는 자기 격리 VM에서도 같은 명령으로 확인한다. 새 회귀의 내부 사건 상한은 2초,
  worker 안전 상한은 5초이며 정상 실행은 5초 이하로 확인한다.
  고정 sleep·실패 재시도·기대값 완화·시한 확대는 없다. 전체 ci-local은 착지 차례에 실행한다.
- OSS 후보는 [portable-pty의 실제 openpty/CLOEXEC 구현](https://github.com/wezterm/wezterm/blob/main/pty/src/unix.rs)
  ([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)),
  [Pexpect의 터미널 모드와 EOF 구분](https://github.com/pexpect/pexpect/blob/master/pexpect/pty_spawn.py)
  ([ISC](https://github.com/pexpect/pexpect/blob/master/LICENSE)),
  [Expectrl의 PTY 수명 관리](https://github.com/zhiburt/expectrl)
  ([MIT](https://github.com/zhiburt/expectrl/blob/main/LICENSE))다.
  모드 준비·입력·자식 종료·EOF를 구별하고 생성 중 FD 상속을 막는 원칙을 따른다.

## coordinator 질문 카드 누락 감지(t-1152, §8.509)

- `turn_reply::tests::both_harnesses_only_publish_completed_main_turns`는 Codex 완료 사건과 Claude 턴 길이 사건을 통해 실제 최종 답만 고르고 도구·하위 agent·중복 완료를 제외한다. `plan::tests::turn_replies_follow_complete_appends_without_plan_changes`는 실제 JSONL을 반만 append한 상태와 완성한 상태를 구별하고, 계획 값이 그대로여도 완료 답을 수집하며 Claude `end_turn`을 성급히 완료로 보내지 않는지 검사한다. 두 시험은 중간 질문·부분 쓰기를 최종 질문으로 잘못 알리거나 계획 변화가 없어 누락하는 회귀를 막기 위해 필요하다.
- `question_guard::tests::user_decisions_are_distinct_from_quotes_and_existing_cards`는 한국어·영어 질문, 조건부 제안·선택 요청·A/B, 코드 fence·인용·간접 보고, 열린 q-ID의 0채움 별칭·닫힌 q-ID·별도 문단의 새 질문을 대조한다. 필요한 이유는 단순 물음표 검색과 메시지 전체 링크 면제가 만드는 오탐·누락을 막기 위해서다.
- `post::tests::question_guard_warns_once_for_both_coordinators_without_changing_turn_or_pushing`와 `question_guard_matches_the_turn_and_checks_existing_card_links`는 실제 Host·DB·post/질문 카드 owner를 쓴다. Claude·Codex 모두 같은 턴의 열린 카드만 인정하고, 이전 턴·다른 머신·worker·이미 답한 링크·역순/중복 완료를 구별한다. 입력 상태·seen과 외부 push가 바뀌지 않음을 검사한다. 실제 접수와 화면상의 글을 혼동하지 않고 재연결 중복과 기존 질문 흐름을 지키는 데 필요하다.
- `post::tests::question_guard_nudges_the_empty_coordinator_once_without_the_regular_idle_delay`는 실제 nudge/submit과 duplex Agent link 응답을 통과한다. 사용자 초안 동안 Input 0회, 빈 입력창에서 일반 유예 없이 안내+Enter 한 번, 실제 working 사건 뒤 같은 묶음 재입력 0회를 확인한다. 필요한 이유는 경고를 inbox에 저장만 하고 coordinator에게 전달하지 않는 누락과 초안 침범을 막기 위해서다. 전체 사건을 5초 안에 끝내고 가짜 agent의 취소 완료를 확인한 뒤 폴더를 정리한다. 새 고정 sleep·재시도·시한 확대·기대값 완화는 없다.
- 부분 확인: `nice -n 15 cargo test --profile verify --locked -p aaa-agent --lib`, `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib`, 두 crate와 proto의 strict clippy, `cargo test --profile verify --locked -p aaa --test deps`, fmt·공개 검사다. 새 시험은 개별 5초 이하이며 새 웹 E2E 번호는 없다. 전체 ci-local·착지는 coordinator의 차례를 기다린다.
- 기존 `link::post_tests::harness_changes_while_waiting_for_welcome_reach_the_host`에도 완료 답을 더한다. Hello/Welcome 사이 완료 답과 idle을 모두 받고, 그 뒤 같은 밀리초의 working도 그대로 전송해야 한다. 기존 idle·진행 보고 계약을 유지하면서 새 답 기억이 상태 칸을 덮어 연결 복구 때 상태를 잃는 회귀를 막기 위해 필요하다.

## routine 차례 확정과 계정 rename의 빈 dispatch 경합(t-1152)

- m13 `routines`의 논리 시계 진행은 tick 완료 뒤 `ping`·`cmd`·`far` 각각의
  `last_run_ms`가 공개한 시각에 도달할 때까지 기다린다. `routine_pass`는 자동화 dispatch
  잠금이 사용 중이면 반환하므로, tick의 종료만 보고 다음 2초를 진행하면 서로 다른 두 차례가
  한 번의 처리로 합쳐질 수 있다. 일반 주기의 처리를 기다리는 동안 논리 시각은 고정한다.
  일정·건너뜀 횟수·승인·로컬/원격 명령·출력·변환 단언과 원래 실패 상한은 유지한다.
- 새 host 회귀 `schedule_dispatch_contention_keeps_the_due_turn_until_it_is_processed_once`는
  실제 Host/SQLite와 dispatch 잠금으로 pass 반환 뒤 미확정 상태, 잠금 해제 뒤 한 번 처리,
  같은 예정 차례의 중복 방지를 검사한다. 필요한 이유는 tick과 일정 처리의 차이를
  실행 부하에 의존하지 않고 보존하기 위해서다. 일정·프로토콜은 바꾸지 않는다.
- 처리할 입력·inflight·이관이 없는 `automation_drain`과 일정형·이관이 없는 `routine_pass`는
  프로젝트 dispatch 잠금을 생략한다. 빈 주기 조회가 일정 처리와 계정 rename의 `try_lock`을
  밀어내지 않게 한다. 처리할 일과 복구가 있으면 기존 잠금 아래에서 상태·정의를 다시 읽는다.
  계정 rename의 실제 충돌 거부 조건은 유지한다. t-1156의 run-5b870199/main d36e7edd도 같은
  잠금의 취득 실패지만 당시 점유자는 미확정이므로 원본 점유 경로를 소급 확정하지 않는다.
- `idle_automation_drain_does_not_join_dispatch_or_block_the_first_account_rename`은
  dispatch를 직접 점유한 뒤 기본 PR 규칙의 빈 drain future를 poll하여 잠금 대기가 없는지 검사하고,
  해제 뒤 첫 rename과 인증 보존을 확인한다. 수리 전에는 첫 poll이 Pending으로 결정적으로 실패한다.
  필요한 이유는 부하가 우연히 맞아야 드러나던 빈 조회의 잠금 요구를 재현하기 위해서다.
- `pending_input_and_migration_still_require_dispatch_and_keep_rename_excluded`는 실제 접수한 입력과
  실제 변환 계획 journal이 있으면 drain이 잠금을 기다리고, 점유 중 rename이 충돌로 거부되며
  금고가 그대로인지 검사한다. 필요한 이유는 빈 조회 최적화가 실행·복구의 배타권을 우회하지 않게 하기 위해서다.
- 참고 후보는 [Tokio 시간 진행](https://docs.rs/tokio/latest/tokio/time/fn.advance.html)·
  [Mutex](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))와
  [robfig/cron의 겹침 건너뛰기](https://github.com/robfig/cron/blob/master/chain.go)
  ([MIT](https://github.com/robfig/cron/blob/master/LICENSE))다.
  시간 진행·호출 반환과 실제 완료를 구별하는 원칙을 따른다. 외부 코드·의존성은 가져오지 않는다.


## 첫 작업 목록의 thread 소속 대기(t-1153, §8.510)

- 기존 E2E24의 실제 host/agent·CLI adopt·WebSocket 사건과 HTTP 목록을 사용한다. 첫 thread HTTP 응답만 `route.fetch` 뒤 사건 gate로 보류하고, 실제 pane state가 DOM에 반영된 뒤 기타/일반 pane 줄 없음·로딩 표시를 확인한다. 빈 전체 목록을 전달하면 일반 pane이 기타에 나타나야 한다. 필요한 이유: 아직 읽지 않은 빈 Map과 읽기를 마친 빈 목록을 구별해야 한다.
- 첫 목록에 503 공개 오류를 전달하면 원인 표시·로딩 종료·기타 없음, 새로고침 뒤 정상 조회 복구를 확인한다. 필요한 이유: 실패를 확정된 빈 목록이나 끝없는 로딩으로 바꾸지 않아야 한다.
- 기존 adopt owner는 빈 옛 목록 보류 중 받은 thread 사건의 줄을 즉시 표시하고, 전체 목록 대기 표시는 유지한다. 해제 뒤 동일 thread 줄 한 개·기타 중복 없음과 기존 role 불변을 확인한다. 필요한 이유: 첫 표시 수리가 §8.218의 사건 보존을 막지 않아야 한다.
- Chromium 1280px·WebKit 390px에서 이미 존재하는 role 없는 adopt thread의 첫 HTTP 응답을 보류한다. 미확정 pane은 기타를 거치지 않고 해제 뒤 thread 줄 하나로 나타나며 일반 셸은 기타에 남아야 한다. 필요한 이유: 초기 응답 순서와 두 엔진의 DOM 적용을 실제로 검사해야 한다. 응답을 가로채는 context는 `serviceWorkers: block`으로 열고 제품 SW는 유지한다.
- 고정 sleep·실패 재시도·시한 확대·기대값 완화·새 E2E 번호가 없다. 실행은 웹 유닛·타입/번들, `NODE_PATH=<작업 폴더 Playwright 1.64.1> nice -n 15 node web/e2e/web.e2e.mjs --spec 24 --workers 1`과 관련 목록/재연결 E2E, 공개 검사다. 내부 공용 브라우저 캐시와 저장소 Chromium launcher만 사용한다. 전체 gate·병합·push는 정규 착지 차례에서 수행한다.
- 부분 검증 결과: 웹 유닛 610개·타입/번들·공개 검사와 E2E24 전체(7.0초), E2E04/13/26 전체(worker 2, 46.8초)가 통과했다. E2E24의 다섯 owner는 각각 0.370/0.480/0.626/0.476/1.669초다. 원래 제품은 새 첫 조회 회귀에서 로딩 표시가 없어 실패했고, 첫 수리판의 기존 「pane 없음」 조기 반환도 같은 회귀가 잡아 완료 목록에만 적용하도록 바로잡았다. 검증 중 load1 59.47→45.90을 관측했으며 시험 재시도·상한 변경은 없다.
- 87회차에서 E2E39의 `settingsPanePreparation`이 첫 응답 보류를 풀기 전에 `pane-bk1`을 기다려 20초에 실패했다. 보존된 e2e.log와 chromium-preparation.json의 초기 phase·두 pane 행 없음으로 대기 순환을 확인했다. 위 E2E39 준비 순서를 새 계약에 맞추며 제품 표시·메뉴 유지·bk1 선택 기대와 시한은 유지한다.
- 모든 E2E의 thread HTTP route·fetch 교체·응답 gate를 찾아 같은 전제를 감사했다. E2E12의 reader 깊은 주소는 hash 설정으로 gate를 풀고, 대량 목록은 목록 DOM으로 완료를 기다린다. E2E24는 초기 pane 숨김과 목록 반영을 이미 검사한다. 나머지 gate는 다른 op만 보류하거나 응답을 관측하므로 첫 threads 목록을 막고 pane 줄을 기다리는 추가 경로는 없었다. 변경 대상은 E2E39 준비 helper 하나이며 기존 owner와 E2E 번호를 유지한다.
- 후속 검증: 같은 제품 실행 파일로 E2E12/24/39 전체가 worker 2, 93.0초에 통과했다(파일별 49.1/3.9/93.0초). 수정한 설정 준비 owner는 Chromium·WebKit 합계 1.545초다. 웹 유닛 610개와 공개 검사가 통과했다. 초기 주소만 제거한 별도 대조는 「첫 threads 목록 반영이 사용자가 연 메뉴를 닫았다」로 실패했고 정규 helper를 바이트 그대로 복원했다. 이 실패는 정규 실행 재시도가 아닌 기존 회귀 검출력 대조다. 제품 소스·기존 시한·기대값·전체 gate는 바꾸지 않는다.


## 화면·로컬 재접속 대기의 사건 관측(t-1117 후속)

- brief와 입력 수락은 화면 조회 전에 pane 사건을 구독한다. 알림 자체를 수락으로 세지 않고 기존 화면·질문·신뢰 창 판정을 다시 수행한다. 출력 묶음은 한 번에 비우고 화면을 한 번 읽는다. 구독 종료/누락에는 기존 관측 간격으로 돌아간다. 운영의 화면 안정3초·paste300ms·수락10초·brief최대3회는 유지한다. 시험의 settle=0은 첫 관측부터 그 설정 그대로 적용한다.
- macOS 로컬 agent는 부모 vnode와 실제 UDS의 dev/inode/birth time으로 새 listener를 구별한다. 실제 socket 교체만 재접속 대기를 깨우며 삭제·일반 파일·chmod·무관한 디렉터리 사건은 계기로 삼지 않는다. 지수 backoff·jitter·상한·인증/Welcome/replay 초기화는 그대로다. reactor는 kqueue fd를 READABLE로만 등록하고 감시 fd를 소유한다. 감시 오류는 단계와 OS 원인을 남기고 기존 backoff로 돌아간다. 다른 OS·원격 연결은 기존 경로다.
- `harness::tests::unhooked_submission_rechecks_output_without_accepting_an_unchanged_screen`은 정지 시계에서 같은 화면 알림의 거부·실제 화면 변화의 즉시 수락·원래300ms 간격을 함께 검사한다. 기존 `unknown_brief_keeps_the_accept_deadline_and_never_retypes_without_a_person`의10초/unknown/수동 재전송 계약도 유지한다.
- `link::local_socket::tests::socket_replacement_wakes_but_other_directory_changes_do_not`은 실제 vnode·UDS를 써서 무관한 파일/권한 변경과 교체를 구별한다. 기존 `authenticated_short_connections_reset_backoff_but_rejections_preserve_it`가 짧은 인증 성공/거부의 서로 다른 backoff 처리를 소유한다. 새 두 시험은 거짓 수락과 불필요한 재접속을 막는 데 필요하다.
- rollout의 사본 실행 파일도 공통 `host_command`를 통해 해당 World의 기존 brief·외부 조회 환경을 유지한다. E37은 각 실제 prepend의 두 frame 뒤 행ID·좌표·진단을 함께 읽으며2400건·모든 쪽·원래행±1px·모든 엔진/폭/harness를 유지한다. 삭제/단계 분리는 없다. 전체계측 전 단일5초미만 결과로 미해결 목록을 줄이지 않는다.
- [Tokio broadcast](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html)(MIT), [libuv vnode 감시](https://docs.libuv.org/en/v1.x/fs_event.html)(MIT), [notify kqueue의 READABLE 등록](https://github.com/notify-rs/notify/blob/main/notify/src/kqueue.rs)(crate선언 CC0-1.0), [Playwright actionability](https://playwright.dev/docs/actionability)(Apache-2.0)를 대조했다. 사건 뒤 실제 조건을 다시 보는 원칙만 적용하고 외부 코드·새 의존성을 가져오지 않았다.


## adopt 가짜 Claude의 입력 표시(t-1117 후속)

- `m13::ADOPT_AGENT`는 입력창을 그린 뒤 커서를 그 안으로 돌리고 canonical tty echo로 Enter 전의 글을 표시한다. 공통 `CLAUDE_AGENT::prompt`와 같은 방식이다. canonical/noecho로 줄 전체만 읽으면 실제 대화형 프로그램과 달리 친 글을 Enter 전에 전혀 표시하지 못해, 제품의 선택 창 재확인 경로가10초 시한을 소모했다. 이어 입력창의 남은 줄 판정이 Enter를 다시 확인하며 같은10초를 소모했다.
- 제품의300ms paste 간격·10초 수락 시한·신뢰/질문 거부·최대시도·동일한 실제 CLI/TLS/PTY/화면 감지를 유지한다. adopt 세 owner의 대화ID·권한거부·보고·재부팅/복원·resolve·폴더/branch 보존 단언과 이름도 유지한다. 시험 삭제·대체 owner 추가는 없다.
- [Rustyline tty](https://github.com/kkawakam/rustyline/blob/master/src/tty/unix.rs)([MIT](https://github.com/kkawakam/rustyline/blob/master/LICENSE))와 [prompt_toolkit renderer](https://github.com/prompt-toolkit/python-prompt-toolkit/blob/main/src/prompt_toolkit/renderer.py)([BSD-3-Clause](https://github.com/prompt-toolkit/python-prompt-toolkit/blob/main/LICENSE))의 입력 중 화면 갱신 원칙을 대조했다. 외부 코드나 의존성을 가져오지 않고 기존 공통 fixture의 tty/커서 배치를 따른다.


- 이미지 보존 owner의 PTY는 `FAKE_AGENT`/`COORD_AGENT`의 줄 단위 custom 보고 셸이며 bracketed paste를 켜지 않는다. Codex 이미지 helper는 그 PTY가 아닌 별도 headless 프로세스다. 이 전용 host의 초기/재시작 명령에만 `AAA_TEST_SUBMIT_GAP_MS=0`을 주어 해당되지 않는 TUI paste 대기를 제거한다. 제품 기본300ms와 실제 paste/수락 경계의 기존 harness owner는 그대로 유지한다. 기존10초 수락 시한·실제2머신/4provider시도·544KiB PNG·모든 canary/권한/재시작/목록/forget/누출 단언은 바꾸지 않는다.

- resolve owner의 dir/worktree/adopted는 원래 생성 순서로 실제 기동한 뒤 각각의 보고·brief와 exit1·resolve를 같은 순서로 검사한다. 기본 한도10 안의 독립된3thread를 준비하며, 보고 대기를 겹쳐도 원래 CLI 호출·단언·ID 생성 순서와 pane/layout/폴더 보존은 그대로다.


## 보고 변경 사건과 owner별 준비(t-1117 후속)

- macOS agent의 보고 감시는 report.md 파일 vnode(제자리 쓰기)와 그 부모 vnode(생성·원자적 rename·삭제)를 함께 관측한다. 감시 등록/metadata 확인은 hash 읽기 전에 한다. 사건은 기존 report hash를 다시 읽을 계기이며, 실제 hash가 달라졌을 때만 기존 ReportChanged를 보낸다. symlink를 따라가지 않고 library 내부는 감시하지 않는다. 원래 크기/권한/hash·호스트 사본 검증을 유지한다.
- 무관한 부모 사건은 보고 stamp가 바뀌지 않으면 끝내지 않는다. 기존1초 점검은 항상 남아 사건 누락·감시 준비/갱신/대기 오류에도 hash를 읽는다. 오류는 단계·OS 원인을 남기고1초 경로로 돌아간다. fd는 CLOEXEC·소유권·READABLE 등록을 지키며 감시 task 종료/교체 때 닫는다. Linux/Windows는 기존1초 경로다.
- `report_writes_and_replacements_wake_without_following_library_or_symlinks`는 실제 vnode·파일로 생성/제자리 쓰기/교체/삭제와 library·symlink 경계를 검사한다. `unrelated_vnode_events_preserve_the_one_second_backstop`은 정지 시계에서 무관한 사건이 원래1초 점검을 앞당기거나 미루지 못하게 한다. 두 시험은 새 사건 관측의 거짓 변경과 점검 유실을 막는 데 필요하다.
- `report_shell_world`는 TUI 없는 FAKE_AGENT의 줄 단위 보고 셸만 쓰는 work_root 재시작·보고 단계/예약 한도·machine_health 세 owner에 한정한다. 이 전용 host에서만 시험 화면 안정/paste 간격을0으로 하고 실제 brief·보고·한도·거부·건강 사건을 기다린다. 일반 World와 CLAUDE_AGENT/PASTE_AGENT의 원래 간격·300ms paste·10초 수락 및 제품3초 화면 안정 기본값은 바꾸지 않는다. 초기/재시작 host에 같은 전용 설정을 유지한다. work_root의 세 실제 worktree는 원래 순서로 만들고 brief도 같은 순서로 확인하되 독립 준비를 겹친다. 모든 branch/dirty/journal/PID/오류 격리 단언을 유지한다.
- E37은 같은 native 목록 click·선택·pane 수명으로 대화 위치를 검사하되 해당 context의 workspace drawer 전환만 즉시 처리한다. 정상 drawer 전환·쓸기·기록/취소 계약은 E39의 기존 `drawerNavigation`이 맡는다. 대화의 실제 두 frame·scroll·모든 HTTP 쪽·2400건·1px·25ms CPU·2초 refresh 정책은 그대로다. 시계 모형이나 단계 분리는 추가하지 않는다.
- 자료 선택 owner는 같은 부모 폴더 준비만 한 번 수행하고 원래2390개 frame을 각각 별도 실제 파일로 만든다. sparse 크기·실제2399경로·200MiB 선택 상한·링크/근거 우선순위·모든 원래 단언을 유지한다. 본문 hash의 실제 읽기는 기존 collect owner가 계속 검사한다. 이관 hash는1MiB 읽기 용량을 유지하는 BufReader로 EOF까지 실제 바이트를 처리하며 작은 파일마다1MiB Vec를 명시적으로0으로 채우는 작업을 없앤다.
- [Playwright fixture](https://playwright.dev/docs/test-fixtures)·[context 내 style](https://playwright.dev/docs/api/class-page#page-add-style-tag)(Apache-2.0), [libuv FS event](https://docs.libuv.org/en/v1.x/fs_event.html)(MIT)·[notify kqueue](https://github.com/notify-rs/notify/blob/main/notify/src/kqueue.rs)(crate선언 CC0-1.0)·[Tokio AsyncFd](https://docs.rs/tokio/latest/tokio/io/unix/struct.AsyncFd.html)(MIT), [표준 BufRead](https://doc.rust-lang.org/std/io/trait.BufRead.html)(MIT/Apache-2.0)를 대조했다. 의미별 fixture 수명·파일/부모의 함께 관측·사건 뒤 실제 내용 확인·표준 스트림 경계 원칙을 따르며 외부 코드나 새 의존성을 가져오지 않았다.

## 고비별 진행 목록과 진행 보고 시각(t-1179, §8.512)

- 기존 `brief_text::tests`는 모든 harness용 brief 구성과 공유 진행 보고 안내를 확인한다. 지침 문자열의 모양만 반복 확인하는 별도 시험은 추가하지 않는다.
- 웹 `summaryCard`의 새 행동 시험은 오래된 요약/최신 진행 보고의 나이 분리, 요약 재갱신, 시각/보고 누락과 음수 시차를 확인한다. 필요한 이유: 활동 보고만 갱신됐는데 작업 목록도 최신으로 오해하거나 없는 시각을 만들어 표시하는 회귀를 막는다.
- 기존 E2E28의 desktop 진행 owner는 실제 pane CLI의 진행률을 바꾸고 파일의 report hash·목록 시각·완료 개수가 그대로인지 검사한다. 실제 파일을 고친 뒤 카드 단계가 갱신되는 기존 단언도 유지한다. 브라우저 시계만 진행해 오래된 표시를 확인하며 고정 sleep은 쓰지 않는다. Chromium 320px·WebKit 390px Reader는 같은 갱신 시각 표시와 기존 읽기 전용·긴 이름·권한 계약을 확인한다.
- 기본 hover 카드의 줄 수는 추가한 갱신 시각 한 줄을 포함해 10개다. 기존 한 줄 표시·높이 280px 상한과 목표/상태/활동 단언은 유지한다. 새 E2E 번호·시험 삭제·재시도·시한 확대는 없다.
- 부분 확인: 웹 유닛 전체·타입/번들, host `brief_text::tests`와 proto 유닛, 두 crate strict clippy·의존 경계·fmt·공개 검사, 저장소 고정 Playwright 1.64.1과 공용 캐시의 E2E28 전체다. 전체 ci-local과 병합·push는 정규 착지 차례에서 수행한다.

## 이관의 같은 부모 폴더 준비(t-1117 후속)

- 큰 inbox owner의 같은 조건 전후 단계 관측에서 inventory는 0.314→0.325초, import는 2.065→2.904초였다. hash 읽기 수리만으로 병렬 import 개선이 입증되지는 않았다. 원래 4002개 실제 원본·CLI inventory/import·응답 상한·정렬/누락·정리는 유지한다.
- Copier는 파일마다 현재 부모가 폴더인지 확인하고 없을 때 기존 create_dir_all을 쓴다. 이미 있는 inbox/done에 mkdir을 반복하지 않는다. 폴더 존재를 장기 보관하지 않으며, 생성·복사·사본 SHA 대조 실패는 원래 Outcome.failed에 남긴다. 파일/본문/해시를 건너뛰거나 생성 실패를 재실행하지 않는다.
- [Rust DirBuilder](https://github.com/rust-lang/rust/blob/master/library/std/src/fs.rs)(MIT/Apache-2.0)의 mkdir/기존 폴더 판정과 [CPython shutil](https://github.com/python/cpython/blob/3.13/Lib/shutil.py)(PSF-2.0)의 _ensure_directory·copytree를 대조했다. 현재 부모를 확인해 준비하는 원칙을 따르며 외부 코드를 복사하거나 의존성을 추가하지 않는다.
- 기존 migrate::convert::e2e::inventory_then_convert가 새/기존 부모에서 실제 복사·바뀐 원본의 해시·제외·설정·DB·같은 대상 거부를 지킨다. 성능 감소와 5초 초과 여부는 다음 전체 차례의 같은 조건 전후 각1회로 판정한다.

## 질문 답 fixture의 실제 입력 수락(t-1117 후속)

- E53 첫 owner는 기존 fake 프로필로 brief·질문 보고를 받은 뒤, 대화 기록을 연결할 때만 가짜 Claude로 전환한다. SessionStart/UserPromptSubmit은 기록·진행 안내용이며 수락 상태를 대신하지 않는다. 실제 foreground job과 PTY 작업 화면을 기존 감지기가 읽어야 한다. 초기 brief까지 Claude 프로필로 바꾸는 실험은 수락 대기 중 카드 답을 거부해 철회했다.
- post-probe 셸의 job control로 가짜 Claude를 별도 foreground 그룹에 둔다. 마지막 명령 뒤 종료 상태를 전파해 셸이 exec로 사라지는 최적화를 피한다. 같은 그룹의 단순 exec는 감지기의 그룹 변경 사건이 아니다. 제품의 조사 주기·수락10초·paste300ms는 그대로다.
- 프로그램은 실제 bracketed paste의 내부 줄바꿈과 바깥 Enter를 구분하고, Enter 뒤 원문 전체를 실제 transcript에 한 번 기록한다. UserPromptSubmit과 작업 화면을 내고, 원래 답 기록 단언 뒤 실제 PTY의 Ctrl+G로 턴을 끝내 입력창으로 돌아간다. 시험용 수락 API·고정 sleep·시계 대역은 없다. 최초 질문/선택/직접 답·사본/권한/동기화·원래 step 이름과 단언을 유지한다. 나머지 E53 owner와 공통 fake 셸은 바꾸지 않는다.
- 기존 E102 foreground 프로그램·E35 bracketed paste fixture와 [Bash job control](https://www.gnu.org/software/bash/manual/html_node/Job-Control-Basics.html), [monitor 옵션](https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html)을 대조했다(Bash GPL-3.0-or-later, 설명서는 GFDL-1.3-or-later). 문서의 원칙을 참고하며 외부 코드를 복사하거나 의존성을 추가하지 않는다.

## PR 조회 owner의 보고 셸 준비(t-1117 후속)

PR 기능 폐지(t-1203)로 해당 owner를 제거하거나 일정 규칙 fixture로 전환했다. 현재 회귀는 아래 t-1203 절을 따른다.

## 대량 보고서 실제 제거의 완료 관측(t-1117 후속)

- E113은 DB·공개 경로의 제거와 비동기 사본 정리를 각각 확인한다. 폴더 사건만 기다리던 관측은 실제 정리 완료·빈 폴더인데도20초 시한을 소모했다. Reaper가 remove_dir_all 성공 뒤 쓰는 기존 host 로그 파일도 깨움으로 관측한다. 성공 조건은 매번 실제 pending 폴더가 비어 있음이며, 로그 문자열이나 응답만으로 성공하지 않는다.
- `bulk-delete-ready.mjs`는 폴더/로그 감시를 모두 등록한 뒤 처음 상태를 확인하고, 성공·오류·원래20초 실패 시한 뒤 두 감시를 닫는다. 고정 sleep·주기 점검·재시도·시한/기대 변경·제품 코드 변경은 없다. 기존601개·500/101 묶음·부분 실패/수동 재개·원본/보관/사본 제거·권한과 UI 응답 계약을 유지한다.
- `bulk-delete-ready.test.mjs`의 새 unit은 실제 파일을 두고 제거 전 폴더 사건과 무관한 host 로그가 성공하지 않음을 검사한다. 마지막 폴더 사건은 보내지 않고, 실제 제거 뒤 완료 로그 사건만 보낸다. 이 한 시험은 관측 유실 회귀를 결정적으로 잡는 데 필요하다. 원래 E113 owner 이름/개수는 그대로이며 새 unit1개·삭제0개다.
- [Node fs.watch 주의사항](https://nodejs.org/api/fs.html#caveats)([MIT](https://github.com/nodejs/node/blob/main/LICENSE))·[libuv FS event](https://docs.libuv.org/en/v1.x/fs_event.html)([MIT](https://github.com/libuv/libuv/blob/v1.x/LICENSE))와 기존 seedHost의 로그 사건 관측을 대조했다. macOS의 파일/폴더 감시 경로가 다름을 참고하되, 이번 실패의 정확한 callback 통합/누락 순서는 미관측으로 구별한다. 외부 코드/새 의존성은 가져오지 않는다.

## 업로드 쓰기 잠금의 상태 폴더 범위(t-1117 후속)

- 전역 올리기 잠금은 서로 다른 상태 폴더의 Host도 직렬화했다. usage cache owner는98회차 평소병렬5.449초·단독0.480초였다. 단독 결과만으로 원장을 줄이지 않는다. 같은 상태 폴더의 쓰기/등록 잠금·마지막 올리기 cache를 Host 수명 동안 함께 소유하고, 독립 상태 폴더는 기다리지 않게 한다.
- registry는 Host::open에서만 canonical 상태 경로를 키로 잠금을 찾고 Weak로 보관한다. `.`·symlink 별칭도 같은 잠금을 쓴다. canonical 경로는 키에만 쓰며 기존 파일 경로/외부 인자는 바꾸지 않는다. 죽은 Host의 항목은 다음 등록 때 제거하고 cache는 Host 수명이 끝나면 버린다. 같은 폴더의 쓰기/게시·합계/파일 수 상한·교체·실패 뒤 재계산·게시 직전 실제 파일 수 확인을 유지한다.
- `independent_upload_states_do_not_block_each_other`는 한 상태의 잠금을 잡은 채 다른 상태의 실제 쓰기와 bytes 확인이 완료되는 사건을 기다린다. 전역 잠금 판에서는2초 경계에서 실패함을 확인했다. 실패에도 잠금을 먼저 풀고 직접 만든 writer를 회수한다. `upload_state_aliases_share_cached_counts`는 같은 상태를 다른 경로로 연 두 Host의 실제 세 파일/합계가 섞이지 않고3/6이 되는지 검사한다. 필요한 이유: 독립 Host의 경합을 없애면서 같은 폴더의 cache 갱신/상한을 깨뜨리지 않아야 한다.
- [Rust Weak](https://doc.rust-lang.org/std/sync/struct.Weak.html)·[Mutex](https://doc.rust-lang.org/std/sync/struct.Mutex.html)·[canonicalize](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)(MIT/Apache-2.0)와 기존 collect_locks의 자원별 잠금 소유를 대조했다. 소유권/경로 정규화 원칙을 참고하며 외부 코드나 의존성을 추가하지 않는다. 새 두 unit은 각각 실제로 한 번 실행됐는지 확인하고, 성능은 다음 전체 차례의 같은 조건 전후로 판정한다.
- 후속 같은조건 병렬에서는 cache owner가0.914→6.704초여서 성능 개선을 입증하지 못했다. 실제1000개 파일 경계는 그대로 두고, 준비용 빈 파일997개는 한 빈 파일과 같은 폴더의 hard link로 만든다. 파일 경로/개수/0바이트·실제 owner의 마지막 추가/초과 거부·교체/실패 재계산은 유지한다. 교체 뒤 다른 빈 파일도 그대로인지 실제 bytes를 검사한다. 원래 단언/상한·기존 owner 이름을 바꾸거나 준비를 시간 밖으로 옮기지 않는다.
- [Rust hard_link](https://doc.rust-lang.org/std/fs/fn.hard_link.html)(MIT/Apache-2.0)와 [CPython os.link](https://docs.python.org/3/library/os.html#os.link)(PSF-2.0)의 같은 filesystem 내 실제 파일 경로 공유를 대조했다. symlink·파일 수 축소·합성 usage는 쓰지 않으며, 테스트의 실제 디렉터리 항목을 유지해 inode 준비 작업만 줄인다. 새 시험·삭제·대체는 없고, 단독 통과로 보통 병렬5초 기준을 통과했다고 판정하지 않는다.

## 질문별 직접 답과 의견(t-1194, §8.517)

- host compose keeper는 선택+의견·직접 답만·원래 질문 번호·여러 줄 원문과 무관한 미응답 거절을 검사한다. 필요성: 한 의견이 다른 질문까지 대신하거나 선택을 덮는 회귀를 막는다.
- proto keeper는 comments 없는 옛 요청·사본을 읽고 null 위치와 줄바꿈을 왕복한다. CLI keeper는 원래 번호·반복 --comment·글 안의 =·빈/중복/없는 번호 거절을 검사한다. 필요성: 이전 공통 text 호환과 질문별 매핑을 지킨다.
- post keeper는 실제 host DB 사본·질문 목록·inbox 본문을 확인하고, repeat keeper는 재배열된 질문에 선택·의견·원래 번호가 함께 따라가는지 검사한다. 기존 q-ID별 부분 답 keeper도 의견 배열과 전체 원문을 확인한다. 필요성: 화면만 바뀌고 기록·전달이 손실되는 회귀를 막는다.
- 웹 유닛 keeper는 직접 답이 자기 질문만 채우며 나머지 미응답을 추천 또는 미완료로 남기는지 확인한다. 실제 React 카드 headless 회귀는 `node web/e2e/question-comments.browser.mjs [그림 폴더]`다. 전후 촬영은 `--before-ref=<기준 git ref>`를 사용한다. Playwright는 기존 NODE_PATH의 고정판이며 설치하지 않는다. 기본 Chromium launcher 옵션을 쓴다. 공용 브라우저가 없는 환경에서는 이미 설치된 headless Chrome을 `AAA_BROWSER_CHANNEL=chrome`으로 명시할 수 있으며 정규 Chromium/WebKit E53 확인을 대신했다고 하지 않는다.
- 카드 회귀는 1280px·390px의 light/dark에서 두 의견과 선택을 보내고 질문별 답 표시·가로 넘침 없음을 확인한다. root capture 중 부모의 동기 갱신을 끼워 실제 textarea 입력 보존도 검사한다. 별도 시나리오는 단일 질문 Enter·Reader 비활성·다른 미응답 거절·부분 답 초안·coordinator post·전송 거부 뒤 원문/재전송 가능 상태를 검사한다. 각 조건은 DOM·callback 완료 사건으로 끝나고 실패 상한 5초를 쓰며 sleep·재시도·기대값 완화는 없다.
- 기존 E53의 아이콘 두 질문 owner는 UI 의견 두 개·선택 B·여러 줄 직접 답을 실제 host로 보내고 task Follow-ups·inbox 본문·답한 카드·ThreadView 사본을 대조한다. 기존 추천 전송·옛 --text·늦은 응답 owner와 E2E 번호를 유지한다. 정규 전체 gate와 Chromium/WebKit E53은 Unix 착지 차례에서 수행한다.
- 참고 후보는 [SurveyJS comment area](https://surveyjs.io/form-library/examples/individual-checkbox-comments/documentation)([MIT](https://github.com/surveyjs/survey-library/blob/master/LICENSE))와 [JSON Forms multiline](https://jsonforms.io/examples/control)([MIT](https://github.com/eclipsesource/jsonforms/blob/master/LICENSE))다. 질문에 붙는 입력 배치만 참고하며 코드나 의존성을 가져오지 않았다.

## 질문별 답 시험 경로와 E02 문자 로케일(t-1194 후속)

- E53의 기존 강조/M3 두 질문 owner는 옛 공통 입력칸 대신 각 질문의 직접 답 칸을 쓴다. 고르지 않은 두 질문에 각각 답하고 여러 줄 의견 원문·두 null pick·comments 사본·thread Follow-ups·coordinator inbox 전체 본문을 대조한다. 같은 두 번째 질문 재보고가 다시 열리지 않는 기존 단언과 형식 경고·선택지 글자·단일 질문/CLI의 옛 text 계약을 유지한다.
- E69는 휴대폰에서 1B 선택과 2번 직접 답을 전하고 실제 DB 사본의 picks/comments·한 번만 남은 inbox의 선택/원문·post 답함·다른 기기의 같은 답을 확인한다. 원본 그림을 지운 뒤 사본 유지·두 번째 답 거부·q-ID/권한/칩/테마 계약도 그대로다. 새 시험·단계 분할·시험 삭제는 없다. 기존 두 owner가 질문별 입력을 통한 원문 보존·전달 계약을 계속 소유한다.
- 기준 main f5e3e1dc를 새 worktree에서 worker 1로 E02 전체 단독 실행했다. SSH의 LANG 빈 값·LC_CTYPE=C 환경에서 부하가 낮아도 같은 한글 줄 20초 실패와 ASCII만 남은 실제 화면이 재현됐다. 질문별 답 변경 전후의 E02 소스는 같았다. 원래 실패를 재시도해 통과한 것으로 세지 않는다.
- 실제 /bin/sh PTY 대조는 C에서 Readline의 convert-meta on·input-meta off와 한글의 meta 키 해석, en_US.UTF-8에서 convert-meta off·input-meta on과 한글 원문 출력을 확인했다. E02의 labEnv만 macOS en_US.UTF-8·Linux C.UTF-8로 정한다. 사용자/OS 로케일·운영 agent 환경은 바꾸지 않고, 원래 한글 문자열·fontOk의 글자 폭·20초 상한을 유지한다. 이 검증은 기존 E02가 맡으며 구현 상수만 확인하는 새 시험은 만들지 않는다.
- 보강한 E53의 첫 실행에서 여러 줄 문자열을 pane wait-output에 넣은 관측 오류가 드러났다. 이 API의 match_output은 text.lines()에서 맞는 첫 줄을 찾는다. 각 화면 줄은 해당 API로 확인하고 사본·Follow-ups·inbox는 원문 전체를 정확히 대조하도록 고쳤다. 오류 로그를 보존했으며 코드 수정 뒤 E53만 추가 검증한다. E02·E69의 통과를 얻기 위한 재실행·시한 확대·원문 단언 완화는 없다.
- 참고는 [GNU Bash Readline init의 convert-meta/input-meta와 LC_CTYPE](https://www.gnu.org/software/bash/manual/bash.html#Readline-Init-File-Syntax)(Bash GPL, 문서 GFDL)와 [Python PTY 표준 API](https://docs.python.org/3/library/pty.html)(PSF-2.0)다. 문자 로케일과 실제 PTY 입력을 대조하는 원칙을 참고했으며 외부 코드·새 의존성을 가져오지 않았다.

## PR 워크플로 제거(t-1203, §8.520)

- 새 m13 `no_pr::reports_and_cleanup_never_query_prs_or_resolve_on_main_membership`는 실제 host/agent와 `gh` 호출 함정으로 보고의 PR 줄·ticker·resolve 모두 조회하지 않음을 확인한다. main 조상이어도 수동 resolve 전까지 열린 thread, PR 필드 없는 Front 응답·옛 routine의 무발화/무오류·main 조상 branch 정리를 함께 검사한다. 필요한 이유는 조회 경로 삭제만으로 상태·정리·기존 규칙을 놓치는 회귀를 잡기 위해서다. 고정 지연 없이 보고 회수와 ticker 완료 사건으로 끝낸다.
- 새 host unit `legacy_pr_metadata_does_not_override_report_groups`는 옛 PR 상태 제거·묶음 호환 읽기와 다른 legacy 자료 보존을 확인한다. 이 검사는 공용 legacy map에 PR 키가 다시 들어가는 회귀를 막는 데 필요하다.
- 새 host unit `retired_pr_sources_are_inert_and_doctor_preserves_their_original_bytes`는 legacy routine/v1/v2 규칙의 목록 제외·새 저장 거부, CI 유지·다른 규칙 변환 가능, doctor의 바이트 보존·반복 무변경과 기본 안내를 확인한다. 필요한 이유는 기존 프로젝트의 PR 규칙이 config-error를 반복하거나 되살아나는 것을 막기 위해서다.
- PR-only m13 `pr_followup`, `merged_pr_resolves_the_thread`, `merged_thread_cleans_its_task`, `merged_resolve_waits_for_the_routine_prompt`, `resolve_looks_up_the_pull_request_once_more`, `pr_environment_failure_is_not_recovered_by_another_thread`, `pr_unpublished_branch_never_calls_gh`, `pr_unauthenticated_machine_is_quiet_and_resumes`, `pr_missing_executable_is_quiet`를 제거한다. 삭제된 common/host PR 모듈 unit도 폐지 기능의 계약이므로 제거한다. 대체는 위 no_pr owner이며, 기존 `task_main_merge`의 원격 main·미병합·보고 누락 계약을 유지한다.
- 변환·사본 충돌·계정 rename·dispatch·E2E40 fixture는 PR 규칙 대신 명시적인 일정 규칙을 쓴다. 완료 보관·E2E86은 main/보고/기존 완료 근거를 쓴다. sidebar unit은 과거 PR 필드가 섞여도 보고 상태만 표시한다. 기존 상태·보존·승인·권한 단언을 유지하며 시한·재시도·기대값 완화는 없다.
- 외부 참고: [Git merge-base 소스](https://github.com/git/git/blob/master/builtin/merge-base.c)([GPL-2.0](https://github.com/git/git/blob/master/COPYING))의 조상 판정과 [GitHub CLI run list](https://github.com/cli/cli/blob/trunk/pkg/cmd/run/list/list.go)([MIT](https://github.com/cli/cli/blob/trunk/LICENSE))의 Actions 실행 조회를 대조했다. 기존 Git/CI 경계를 재사용하며 외부 코드나 의존성을 추가하지 않는다.

## PR 제거 착지 실패의 단독 대조와 fixture 수리(t-1203)

- main `5211848d`와 PR 제거 후보 `383935ff`의 원본 m13 두 owner를 nice15로 하나씩 실행했다. 신뢰 질문 owner는 두 판 모두 통과했고 보고 상태 owner는 main 통과·후보의 원래 시한 실패였다. 재시도로 통과를 채택하지 않고 각 판의 첫 결과를 보존했다.
- `report_events::report_state_precedes_completion_event`는 사건 발행 잠금의 계기로 PR routine을 토글했다. 후보는 폐지된 규칙을 목록에서 제외하므로 toggle이 즉시 `BadRequest`로 끝나 Hhold를 내지 못했다. fixture만 명시적인 `every 1h` 일정 규칙으로 이관한다. 원자 공개·H/W/R 사건·상태 저장 선행·실시간/재생 순서의 모든 단언과 원래 시한을 유지한다.
- 원본 CI의 추가 `(trust, -)`는 brief 화면 관측이 연 질문에 답한 뒤 첫 agent Question 사건이 늦게 와 같은 질문을 다시 열고 Idle에서 닫은 기록이었다. `harness.rs`·`brief.rs`·보고 owner는 main과 실패 후보 383935ff에서 바이트가 같다. 별도 단독 진단에서 main의 실제 Host/DB에 Idle→brief 질문→답→지연 Question→Idle 순서를 재생하자 같은 두 trust 기록이 생겼다. PR 제거 전에도 존재하는 경쟁임을 확인했으며 이 범위에서는 제품의 질문 처리·원래 E2E 기대값을 바꾸지 않는다. 진단 source는 원본으로 복원했고 실패 재현 자료는 thread library에 보존했다.
- 착지 충돌 해소 때 FRONT_API 문서 주석 사이에 넣은 빈 줄을 제거한다. API·계획 번호와 E30·35 분할은 유지한다. 전체 ci-local·cross·Windows production·push는 계측 종료 후 새 착지 차례에서 수행한다.

## 느린 owner의 선택 단계 계측(t-1206)

- Git/CI source가 진행 중이면 next_due 후보에서 빠진다. 완료 뒤 새 deadline이 생겨도 ticker를
  깨우지 않으면, 이미 고른 일반 ticker 간격 동안 대기가 남았다. 완료 시 잠금을 푼 뒤 기존
  tick_wake로 deadline을 다시 계산하게 한다. 조회 간격은 여전히 완료부터 세고 중복 실행을 막는다.
  깨움은 next_due를 쓰는 CI/Git에만 적용한다. usage/automation은 주기0이므로 공통 완료에서
  깨우면 ticker→완료→ticker가 반복된다. 이 두 source는 기존 변화/할 일 사건으로만 깨운다.
  새 `completed_source_rearms_a_sleeping_ticker_without_skipping_the_interval`은 정지 시계에서
  이 깨움·완료 기준 deadline·즉시 중복 조회 거부와 주기0 무깨움을 함께 검사한다. 수리 전0.57초에 실패했고
  수리 뒤 기존 시간 경계와 함께0.45초에 통과했다. 오래된 timer를 깨우지 않는 회귀를 막기 위해 필요하다.
- GitGate의 세 preparation owner는 모두 TUI 없는 FAKE_AGENT를 쓴다. 이 fixture의 화면 안정과
  paste 준비만0으로 하며 실제 Git 완료·시한·원문/journal·재시작·폴더 보존 단언은 유지한다.
- `AAA_TEST_PHASES`를 자기 작업 폴더의 새 절대 경로로 지정하면 Rust와 웹은 owner별 JSONL을
  남긴다. `start/phase/end`의 단조 경과와 실제 사건 시각으로 준비·계약·관측·정리를 구별한다.
  Rust guard는 실험실보다 먼저 선언해 정상 마지막 cleanup 표지가 실제 Drop까지 포함한다.
  panic·실패·중단의 부분 원문도 남기며, 계측 I/O 오류는 원래 실패를 바꾸지 않는다.
- rollout 3개는 사본 생성/서명·host/agent·coordinator 준비와 실제 전이·정리를 나눈다.
  Git/CI는 실제 poll/304·ticker 울타리를 별도로 표시한다. 새 Rust 경계 3개·cache·이미지 복원은
  준비/실제 계약/재시작/정리를 기록한다. 상태·호출·상한·원래 단언은 유지한다.
- 웹 step의 원래 이름/개수는 그대로다. 공통 pairPage·shot·대화 진단 body의 보조 span을 남긴다.
  E37은 기존 왕복 안에서 읽은 브라우저 두 frame 구간과 Node 왕복을 각각 기록한다.
  E53의 foreground 준비/답 PTY 수락, E68의 실제 reload gate/TCP/앱·agent·PTY 복귀,
  E94/E102의 조합별 준비·입력·정리도 구분한다. body 읽기 span은 본문 수신과 protocol 대기까지
  포함하므로 JSON 순수 CPU 비용이라고 해석하지 않는다.
- `python3 scripts/case_phases.py <새 계측 폴더>`는 원문을 바꾸지 않고 요약한다.
  `inclusive_spans`와 `browser_samples`는 phase 안에 겹치는 보조 값이므로 전체에 더하거나 빼지 않는다.
  helper가 끝나기 전에 닫힌 경우 `unfinished_spans`를 남긴다. 이것만으로 배경 진단의 완료를 주장하지 않는다.
  개별 wall은 준비·관측·정리를 모두 포함하며, 파일/전체 suite wall과 구별한다.
- 독립 성능 판정 예:
  `python3 scripts/case_phases.py <새 계측 폴더> --require usage_cache_keeps_limits_replacements_and_failed_writes_correct --max-seconds 5`.
  필수 owner 미실행·빈 폴더·실패/미완료·상한 초과는 실패다. 실패 회차와 성공 회차를 같은 폴더로 합치지 않는다.
  성능 판정을 통과한 뒤에만 별도 정규 ci-local을 시작하며, 정규 gate의 기준은 바꾸지 않는다.
- E90 Reader는 같은 엔진·폭의 밝음/어두움에서 같은 신원/context를 재사용하는 후보다.
  2엔진×4폭×2테마의 16조합을 그대로 검사하며, 두 번째 테마는 실제 공유 설정을 바꾸고 media를
  맞춘 뒤 원래 URL로 문서를 새로 연다. Reader pairing/context 준비는 16회에서 8회가 된다.
  모든 기존 drawer·필터·URL·가로 넘침·footer·기준 실행·오류·사진 단언을 유지한다.
  단계 분할은 없다. Mac 단독 전체 E90은 통과했으며, 실제 시간 개선과 느린 원장 수는 같은 조건의 전후 계측으로 판정한다.
- 새 keeper는 위 재예약 경계와 계측용3개를 합한4개다. Rust의 unwind 원문 보존, Node의 동일 오류·보조 관측/정리 보존,
  Python의 미실행/부분 결과 거부와 겹치는 span의 이중 계산 방지를 각각 지켜야 하므로 필요하다.
  시간 경과를 기다리는 시험·삭제·대체·새 E2E 번호는 없다.
- 참고 후보: [Playwright fixtures](https://playwright.dev/docs/test-fixtures)의 의미별 수명과
  [media 전환](https://playwright.dev/docs/api/class-page#page-emulate-media)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)),
  [Rust Instant](https://doc.rust-lang.org/std/time/struct.Instant.html)의 단조 경과
  ([MIT/Apache-2.0](https://github.com/rust-lang/rust/blob/master/COPYRIGHT))다.
  fixture 수명과 시각 원칙을 참고했으며 외부 코드/실행 의존성을 추가하지 않는다.
- Windows의 부분 통과나 0개 실행을 Mac의 단독/평소병렬 결과로 바꾸지 않는다.
  Mac 전용 worktree에서 같은 기준·동일 조합·nice15·Rust6worker/4thread·웹10worker·빌드별도로
  전후 각1회와 관련 전체 E2E를 확인한다. 무거운 전체 전후는 coordinator의 계측 차례를 받아 실행한다.

## inbox 안내 말풍선의 연한 글색(t-1199)

- E2E102는 주입된 `[aaa inbox]` 말풍선의 계산된 글색이 같은 대화의 사람 말과 다른지 확인한다. 필요한 이유: 표지만 남고 본문이 사람 말과 같아지는 회귀를 실제 host 대화 DOM에서 잡는다. 새 E2E 파일·단계는 없다.
- `theme.test.mjs`는 12개 팔레트 자료의 `--text-quiet`가 말풍선 바탕(`--bg-elevated`)에서 4.5:1 이상인지 확인한다. 기본 9개 팔레트는 CSS 섞기 값이라 전후 촬영 때 headless Chromium의 계산된 색으로 대비를 쟀다(최저 catppuccin 밝음 5.14:1).

## diff의 접힌 맥락 펼치기(t-1202, plan §8.524)

- `cargo test --profile verify --locked -p aaa-agent --lib diff::tests::`의 새 `full_context_is_file_only_bounded_and_keeps_default_patch_small`은 실제 git rename의 기본 patch가 작게 남고, 전체 맥락에는 첫·마지막 줄이 있으며, 원본 index·ref·파일이 바뀌지 않고 상한 초과/파일 없는 요청은 거부됨을 본다. 필요한 이유: 맥락 펼치기가 기본 표시·읽기 전용·전송 상한을 깨지 않아야 한다.
- `node --test web/test/diffContext.test.mjs`의 세 keeper는 실제 git patch로 CRLF·마지막 개행 없음, 구판/잘림/기준 변경 거부와 기존 hunk 밖의 새 변경 거부를 검사한다. 필요한 이유: 불완전하거나 서로 다른 시점의 원문을 hydration에 넘겨 변경을 숨기지 않아야 한다. 원문 합성 parser를 따로 만들지 않고 기존 라이브러리를 쓴다.
- 기존 E2E42의 160줄 파일에서 처음에는 변경 주변만 보이고 전체 맥락 요청은 0회여야 한다. 데스크톱 Chromium과 iPhone WebKit의 새 두 단계에서 실제 기본 UI를 데스크톱 Enter·휴대폰 tap으로 눌러 20줄을 더 보이고, 전체 펼치기로 첫·마지막 줄을 확인한다. 이어지는 펼치기는 추가 요청 없이 같은 원문을 쓴다. 필요한 이유: 가상화·shadow DOM·지연 hydration의 실제 연결과 휴대폰 터치 크기는 순수 유닛만으로 확인할 수 없다. 기존 카드·기기 폭·테마·대량 lazy 계약과 E2E 번호는 유지한다.
- 고정 sleep·재시도·시한 확대·기대값 완화는 없다. WebKit에서 SVG use가 터치 대상을 가리므로 아이콘은 pointer-events: none으로 하고 단추 자체에 실제 tap을 보낸다. 새 개별 유닛/단계는 보통 부하 5초 이하를 확인하며 전체 suite 시간과 구별한다.

## 작업 기록 통합 검색(t-1205, §8.527, E2E118)

- host `record_search::tests` 네 계약은 실제 SQLite·프로젝트 파일·보고 판 보존 API를 사용한다. 필요한 이유는 최신 보고 덮어쓰기 뒤 과거 r 판 열기, 답과 직접 의견 보존, 할 일 current/before/after 중복 제거, 정리·재시작·프로젝트 경계, 거른 뒤 커서 페이지와 한글·문자 그대로 검색을 함께 지켜야 하기 때문이다. 추가로 같은 보고 본문의 여러 r 별칭, 마지막 출처 삭제, 1MiB를 넘는 원문의 UTF-8 조각/응답 상한/완전 복원을 검사한다.
- E2E118은 실제 host 보관 DB·파일에 보고 34판·답한 질문·할 일 변경 본문을 준비하고 실제 CLI 검색/원문을 대조한다. Chromium desktop과 WebKit 390px Reader가 모든 종류 30+7건, 종류 필터, 과거 보고·선택과 직접 답, 현재와 다른 할 일 메모 원문을 연다. 필요한 이유는 Rust 저장 계약만으로 웹 종류 선택·다음 페이지·원문 전환과 Reader의 실제 HTTP 접근을 확인할 수 없기 때문이다. 준비와 각 브라우저 계약을 나눠 개별 5초 미만을 확인한다.
- 실행: `cargo test --profile verify --locked -p aaa-host --lib record_search::`, `cargo test --profile verify --locked -p aaa-proto`, `cargo test --profile verify --locked -p aaa --test deps`, 웹 유닛·타입/번들, 고정 Playwright 1.64.1과 공용 브라우저 캐시에서 `node web/e2e/web.e2e.mjs --spec 118-record-search`다. 새 도구 설치는 작업 폴더의 Playwright npm 패키지뿐이며 브라우저·시스템 설정은 바꾸지 않는다.
- 기존 시험 삭제·단언 완화·재시도·고정 sleep·시한 확대는 없다. 전체 ci-local·cross와 main 병합/push는 착지 창에서 수행한다.

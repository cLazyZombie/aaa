# E109 WebKit 초기 앞 기록 대기 수리

## 실패 근거와 이전 수리

- [실패 job](https://github.com/cLazyZombie/aaa/actions/runs/42539284534/job/128910391060)의 전체 로그와
  `web-e2e-result-42539284534-1` artifact를 확보했다. 실패 SHA는 `c9119f6a`다.
- t-1462의 p-00001901은 수리 배정 요청이며 수리 SHA가 없다.
  [이전 job](https://github.com/cLazyZombie/aaa/actions/runs/41428575946/job/125221120607), SHA `0e481e12`의
  artifact도 확보했다. 두 실행의 `return-failure-390.json`은 같은 초기 상태·같은 소스 줄에서 실패했다.
- 기존 `2b664b6e`(초기 viewport 준비 확인)와 `425e602a`(최초 오류·수명 기록)는 실패 SHA의 조상이다.
  적용 누락으로 분류하지 않는다. 이후 같은 서명을 수리한 별도 SHA는 검색 기록에서 확인되지 않았다.

## 실패 대상과 사건

공용 FAIL 줄의 1280×800 화면은 준비 단계의 Chromium 셸이며 본문도 `t1-42`다.
실패한 WebKit coordinator와 pane이 다르다. WebKit 전용 artifact의 launcher는 `webkit-2370`이며
`check(engine, 390)`의 controller에서 기록했다. 실패 전용 스크린샷은 원본 artifact에 없다.

| 사건 | 두 실패에서 확인한 값 |
| --- | --- |
| snapshot | 앞서 Chromium에서 쓰던 112×40 화면 |
| resize·state | WebKit의 47×38로 변경 |
| 준비 통과 | readonly=false, fit=same, historyMore=1, historyWaiting=0 |
| 이동 직전 | viewTop=viewBase=2002, rowHeight=19, scrollTop=38038 |
| viewport 크기 | scrollHeight=38760, clientHeight=722 |
| 실패 predicate | `historyApplied > 0` 대기, 기존 E109 146행 |
| 실패 상태 | historyApplied=0, historyRequests=[], scrollTop=38038, viewTop=2002 |
| 정리 | timeout 뒤 cleanup.start→page.close→context.close→정상 browser exit |

Chromium의 일반 이동·Ctrl+Tab 6회, 실제 입력, 숨김/만료, Reader 단계는 통과했다.
WebKit은 최초 앞 기록 준비에서 실패해 왕복·실제 입력·Reader에 진입하지 않았다. PTY 휠 수 7은
앞서 실행한 Chromium과 공유한 fixture의 누적값이며 WebKit 입력 성공 증거가 아니다.
heartbeat나 작업 종료 시점 load로 환경 원인을 단정하지 않는다.

## 원인과 수리

기존 fixture는 `waitForFunction`의 rAF 안에서 준비를 확인하고 `scrollTop=0`도 썼다.
이미 DOM과 버퍼가 같은 맨 아래를 가리켜도 같은 frame 뒤쪽의 xterm viewport 동기화가 남을 수 있다.
그 동기화가 native scroll 사건 전에 맨 아래를 다시 쓰면 두 이동이 합쳐지고,
xterm과 제품의 앞 기록 요청기는 맨 위 이동을 받지 못한다. 이후 출력이 없는 fixture는 기다리는 사건을 만들지 않는다.
원본에는 개별 rAF callback 기록이 없으므로 이 callback 순서를 원본에서 직접 관측했다고 주장하지 않는다.
다음 결정적 대조는 같은 요청 0회·맨 아래 고정 상태를 만들고, 해당 경합만 제거하면 통과한다.

`historyScrollReady`는 rAF에서 읽기만 한다. `scrollHistoryWhenReady`는 받은 viewport handle에
별도 브라우저 task로 한 번만 `scrollTop=0`을 쓴다. native scroll 처리가 다음 rAF 전에
실제 xterm 버퍼를 옮긴다. 최초 앞 기록·숨기기 전 앞 기록 두 호출에 같은 경계를 적용한다.
제품 코드·기존 30초 대기·일반 이동/Ctrl+Tab/휠/실제 입력/Reader 단언은 바꾸지 않는다.

## 회귀 검증

- `web/test/xterm-source.test.mjs`의 기존 upstream 메서드 probe는 준비 확인이 DOM을 쓰지 않는지,
  크기·viewport 준비 전에는 기다리는지 검사한다. 기존 스크롤·연속 키·해제 회귀를 보존하며 7건 통과했다.
- `web/e2e/history-scroll.test.mjs`는 실제 패치 xterm과 Chromium 1280·WebKit 390을 쓴다.
  실제 rAF predicate 직후 xterm viewport 동기화를 실행해 순서를 결정한다. 좌표·buffer·native 사건은 위조하지 않는다.
  구판은 두 엔진 모두 top=30045, row=2003, 맨 위 native 사건=0으로 RED다.
  수리판은 top=0, row=0, 맨 위 native 사건=1, 0 쓰기=1로 GREEN이며 각 조합 1초 미만이다.
  여기의 사건 수는 실제 host 요청 수가 아니다. 실제 host의 앞 기록 적용은 기존 E109에서 검사한다.
  준비 상태와 입력의 native 사건 도착을 구분하기 위해 필요한 keeper다.
- 최초 keeper 실행은 `once` 관측기가 앞선 맨 아래 scroll에 해제돼 맨 위 사건 수만 0으로 실패했다.
  모든 native 사건을 관측하도록 고치고 같은 기대값을 유지했다.
- 기존 `return-diagnostics.test.mjs` 두 WebKit keeper도 통과했다. 최초 오류·정리 오류의 독립 보존을 유지한다.

## 개발 검증 결과

검증은 모두 `aaa verify -- <명령>`으로 실행했다. 작업 폴더에 Playwright 1.64.1 패키지만 설치하고
브라우저는 기존 공용 캐시를 사용했다. 실제 host·agent 실행 파일은 이 worktree에서 새로 빌드한 사본이다.

| 명령·owner | 결과 |
| --- | --- |
| `node --test web/test/xterm-source.test.mjs` | 7건 통과 |
| `node --test web/e2e/history-scroll.test.mjs web/e2e/return-diagnostics.test.mjs` | 4건 통과 |
| `bash scripts/ci-local.sh --only web,e2e,public --spec 109` | 타입·번들, 웹 유닛 717건, E109 전체 조합, 공개 검사 통과 |

E109는 Chromium 1280과 WebKit 390에서 기존 일반 이동·Ctrl+Tab 6회, 실제 입력, 숨김 기록 적용·출력,
5분 만료·새 snapshot 입력 차단, Reader 복귀·비전송을 모두 통과했다. 두 조합은 각각 5.45초·8.27초였으며,
시나리오 수·시한·단언은 유지했다. 새 native keeper는 최종 명칭 정리 뒤에도 두 엔진 모두 통과했다.

개발 검증은 미커밋 상태라 workspace 원장에는 수집되지 않았고 로컬 검증 로그는 `target/verification/t-1659/`에 있다.
공유 helper가 변경돼 착지 범위 판정은 full이다. 여기의 관련 웹 검증을 전체 착지 gate 통과로 표시하지 않는다.

## 참고한 공개 구현

- [xterm 5.5 Viewport](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)
  ([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)): 예약된 `_innerRefresh`의 DOM 동기화를 대조했다.
- [Playwright waitForFunction](https://playwright.dev/docs/api/class-page#page-wait-for-function)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 기본 rAF polling을 확인했다.
- [HTML rendering 순서](https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering): scroll 처리와 rAF 순서를 참고했다.

외부 코드·제품 의존성 추가, sleep·재입력·재시도·시한 연장·기대값 완화·시험 삭제는 없다.
착지 gate는 승인된 착지 창의 최종 통합 SHA에서 따로 실행한다. 원본 GitHub CI의 회복은 아직 확인하지 않았다.

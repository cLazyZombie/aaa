# E109 복귀 본문 관측의 덮개 오탐 수리

## 원본 실패와 확인 범위

[원본 macOS job](https://github.com/cLazyZombie/aaa/actions/runs/43620151649/job/132523803409)의
`web-e2e-result-43620151649-1` artifact를 확보했다. 기준 main은 `977dbacd`다.
실패 predicate는 E109의 `복귀 중 거짓 읽기 전용·빈 본문`이며 실제 불량 목록은
`thread/ctrl-tab/0` 한 건이었다. 마지막 진단 phase인 `reports/ctrl-tab/0`과 실패 전환을 혼동하지 않는다.

같은 Chromium controller에서 앞 기록 요청 2회가 관측됐다. 두 번째 요청 부근에는
viewport가 0으로 이동한 사건과 history 응답·버퍼 재구성이 있다. 여섯 왕복의 실제 휠·입력은
모두 통과한 뒤 집계 단언에서 실패했다. 연결·ACK 성공만으로 본문 정상까지 판정하지 않았다.

원본은 전체 왕복 표본을 실험실 루트에 저장했지만 CI는 `diagnostics/`만 복사했다.
따라서 artifact의 마지막 `returnFrames`는 보고서 복귀의 정상 표본이며, 불량인 thread 복귀의
`before`·`samples`는 없다. 원본에서 읽기 전용 표지가 떴는지, 본문 감지가 실패했는지 또는 둘 다였는지는
이 자료만으로 확정할 수 없다. 원본의 스크롤 0 이동 원인도 확정하지 않았다.

## 확인한 시험 결함과 수리

제품의 `Term.tsx`는 앞 기록을 붙이기 전에 `.xterm-screen`을 복제한
`.term-history-cover`를 덮는다. 뒤쪽 xterm을 reset하고 기록을 다시 쓴 뒤, 올바른 줄의 실제
`onRender`에서 덮개를 뗀다. 그동안 보이는 본문은 덮개다.

기존 E109는 첫 `.xterm-rows`만 읽어 덮개 뒤의 reset된 DOM을 빈 본문으로 판정했다.
수리한 관측기는 덮개가 있으면 덮개 본문을, 없으면 실제 xterm 본문을 읽는다.
`fake>` 존재 조건·읽기 전용 감지·시한·전환 수·입력·Reader 단언을 유지한다.
단순히 두 본문 중 하나에 글이 있으면 통과시키지는 않는다. 빈 덮개 뒤에 정상 본문이 있어도 실패한다.
제품 소스와 권한 전이는 바꾸지 않았다.

왕복마다 `before`·`samples` 전체를 `diagnostics/pane-return-<engine>-<width>.json`에 저장한다.
표본에는 mutation/frame 출처, 실제/덮개 본문 유무, 표시한 끝부분, viewport·터미널 dataset을 넣는다.
다음 실패에서는 마지막 전환뿐 아니라 집계가 지목한 왕복의 실제 값을 함께 확보할 수 있다.

## 결정적 대조

`web/e2e/return-observation.test.mjs`는 E109의 실제 `instrument()`를 추출해
제품의 패치 xterm과 덮개 CSS로 실행한다. 좌표·버퍼·native 사건은 위조하지 않는다.
실제 xterm `reset()`이 본문을 지우는 동안 같은 사각형의 덮개에는 `fake>`가 남아 있다.

| 조건 | 원본 관측기 | 수리 관측기 |
| --- | --- | --- |
| 본문을 담은 덮개 + 뒤쪽 reset | 두 엔진 모두 빈 본문 오탐(RED) | 두 엔진 모두 정상(GREEN) |
| 덮개 없는 실제 빈 본문 | — | 빈 본문 감지 |
| 빈 덮개 + 뒤쪽 정상 본문 | — | 빈 본문 감지 |
| 읽기 전용 banner | — | 읽기 전용 감지 |

Chromium 1280·WebKit 390의 최종 keeper는 각각 0.34초·0.74초였다.
새 음성 사례의 첫 실행은 write 콜백 직후 DOM을 읽어 실패했다. 콜백은 파싱 완료이므로
실제 본문 그리기를 기다리도록 준비 순서를 고쳤다. 기대값과 시한은 유지했다.

순수 xterm에서 숨김 출력 뒤 스크롤 이동도 관측했으나, 실제 앱의 기존 맨 아래 보정이
같은 조건을 막았다. 제품 결함으로 일반화하거나 추정 수리를 넣지 않았다.
원본 E109 기본 실행, 직렬 6회, Chromium 4배 지연 1회, 4배 지연·6 worker 6회,
4배 지연·120번 왕복에서도 원본 집계 실패는 재현되지 않았다.
이 통과를 원본 CI의 회복 증거로 사용하지 않는다.

## 관련 owner 검증

검증은 모두 `aaa verify -- <명령>`으로 실행했다. Playwright는 작업 폴더의 1.64.1 패키지와
기존 공용 브라우저를 사용했다. 전역 설치·브라우저 설치·시스템 설정은 바꾸지 않았다.

- `node --test --test-concurrency=1 web/e2e/return-observation.test.mjs web/e2e/return-diagnostics.test.mjs`:
  최종 4건 통과. 실제 빈 본문·빈 덮개·읽기 전용 감지와 최초 오류·정리 기록을 확인했다.
- `bash scripts/ci-local.sh --only web,e2e,public --spec 109`: 타입·번들, 웹 유닛 720건, E109 전체 조합, 공개 검사 통과.
  Chromium 1280은 5.78초, WebKit 390은 9.31초였다. 일반 이동·Ctrl+Tab 6회, 실제 휠·입력,
  숨김 기록·출력, 5분 만료·새 snapshot 전 입력 차단, Reader 복귀·비전송을 모두 유지했다.
- `node --test web/test/e2e-layout.test.mjs`: spec 수집·단계·export 계약 6건 통과.
- 개발 검증은 미커밋 상태여서 workspace 원장에는 수집되지 않았다. 로컬 로그는 `target/t-1883/`에 보존했다.
- keeper는 기존 `history-scroll.test.mjs`처럼 독립 실행 owner다. 기본 E2E 수집은 번호 spec만 실행한다.

새 native keeper 경로는 `web/e2e/specs/` 밖이므로 현재 `verify.py`의 착지 범위 판정은 full이다.
개발 owner 통과를 전체 착지 gate 통과로 표시하지 않는다. 착지 창의 최종 통합 SHA에서 gate를 수행한다.
원본 CI 회복과 원본 불량 표본의 정확한 원인은 아직 미확인이다.

## 참고한 공개 구현

- [xterm 5.5 Viewport](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Viewport.ts)
  ([MIT](https://github.com/xtermjs/xterm.js/blob/5.5.0/LICENSE)): 스크롤·reset과 렌더 분리를 대조했다.
- [Playwright waitForFunction](https://playwright.dev/docs/api/class-page#page-wait-for-function)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 실제 DOM 준비 사건을 기다리는 기존 방법을 따랐다.

외부 소스 코드는 가져오지 않았다. 새 의존성·고정 sleep·재입력·재시도·시한 연장·기대값 완화·시험 삭제는 없다.

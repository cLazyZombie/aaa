# E2E104 대화 수명 CI 실패 조사(t-1298)

## 판정과 자료의 한계

[main e5f50d58의 macOS 웹 E2E job](https://github.com/cLazyZombie/aaa/actions/runs/40806979775/job/123228160350)은
`104-chat-lifecycle`의 Chromium 1280 단계에서 실패했다. 시험 fixture가 재검증 시 ID 순서 계약을 어겨 같은 단계·locator의 실패를 결정적으로 재현했다.
이를 수리했다. 원래 CI의 DOM은 없어 당시 동일한 재검증 사건이 있었는지까지 직접 확인할 수는 없다.
로컬 성공을 해당 CI의 회복이나 간헐 확정 근거로 삼지 않는다. GitHub run은 재실행하지 않았다.

job 전체 로그, check-run annotation 전문, `web-e2e-result-40806979775-1` artifact를 읽었다.
artifact에는 `e2e.log`, `e2e-binary.log`, workspace 요약, 전체 실행 시각과 다른 spec의 진단만 있었다.
E2E104의 호출 stack·trace·screenshot·HTTP 응답·chat DOM·host/agent 로그는 보존되지 않았다.
annotation도 같은 오류 요약만 반복한다.

확정한 실패 locator는 coordinator pane의
`.chat-row:last-child` 안 `기록 2399 ·`를 기다리는 `locator.waitFor`다. 상한은 원래 30초다.
이는 첫 `aria-busy=false` 대기 이후의 본문 대기다. 기존 `retention-open-failure` catch는 첫 대기만 감싸서
이 실패의 chat 상태와 stack을 남기지 못했다. 공통 harness의 뒤이은 terminal 진단은 숨긴 xterm과 연결 사건만 보여 준다.

## 보존된 사건 순서

| 자료 | 관측 |
| --- | --- |
| job 시작 | 02:39:38.323 UTC, worker 2, load 40.48/30.43/18.63 |
| 최초 LRU·만료·숨김·복원 | 뒤의 listener 측정까지 도달했으므로 그 앞 단언은 통과 |
| 20회 왕복 뒤 측정 | listener 118→118, term 1→1, timer 1→1 |
| pane WS 약 625.5초 | coordinator 연결 뒤 일반 두 pane으로 차례로 이동 |
| pane WS 약 627.3초 | 마지막 일반 pane을 숨김 |
| pane WS 약 927.4초 | 앞 연결이 닫히고 coordinator를 다시 연결 |
| 실패 | coordinator 마지막 `기록 2399`의 visible 대기 30초 초과 |
| job 끝 | 02:40:38.361 UTC, 60.038초, load 32.52/30.06/19.30 |

WS 시각은 Playwright로 전진시킨 페이지 시계다. 실제 15분을 기다린 것이 아니다.
측정 뒤 세 대화의 용량 시험과 다음 5분 만료를 지난 coordinator 재연결이라는 순서상,
단독 9 MiB 대화를 여는 `open(ids[0])`가 실패 후보 중 가장 좁게 맞는다.
정확한 호출 줄이 있는 stack은 없으므로 이를 원문 stack의 확인으로 표현하지 않는다.
본문 부재·가림·다른 마지막 자식·renderer 미응답 중 어느 상태였는지도 알 수 없다.
시작/끝 부하는 높은 값이지만 실패 순간의 CPU·메모리 자료가 없고, 이 값만으로 환경 원인으로 분류하지 않는다.

## 변경

제품 코드와 기능 기대값을 유지하며 E2E104의 fixture와 진단을 고친다.

agent의 실제 ID는 `history:byte(20자리):block(10자리)`다(`crates/aaa-agent/src/chat_pages.rs`).
`mergeChatPage`는 첫 응답은 배열 순서로 쓰고, 같은 history를 재검증하면 ID순으로 병합한다.
용량 fixture는 첫 배열을 `[생각, 마지막 답]`으로 만들면서 생각의 ID를 `history:budget`으로 넣었다.
이 ID는 실제 마지막 답의 숫자 byte ID보다 뒤로 정렬된다. 따라서 재검증이 본문 대기 사이에 들어오면
마지막 행이 생각으로 바뀌고, 마지막 행에 `기록 2399`가 나타나기를 기다리는 원래 locator가 영원히 맞지 않는다.
실제 agent의 ID 계약에 맞춘 제품의 정렬은 옳으며 fixture가 잘못됐다.

생각 ID를 `history:00000000000000000000:0000000000`으로 바꿔 첫 응답과 재검증 모두에서 답보다 먼저 오게 한다.
단독 9 MiB를 여는 회귀에서는 가상 시계로 10초 backstop을 진행하고 실제 응답의 본문 표지가 그려진 뒤
원래 마지막 답 locator를 검사한다. 이 표지는 시험 응답의 답 뒤에 덧붙이므로 `기록 2399`와 pane 확인을 그대로 지킨다.
시한·용량·LRU·만료·숨김·소켓·누수 허용치를 줄이거나 늘리지 않는다.

진단 누락 보완:

- 각 시나리오와 `open`의 탐색/준비/마지막 본문/제어권/frame/본문 확인을 구분한다.
- 요청 번호·시작 단계, 실제 host 응답과 fixture 가공 후 응답의 history/cursor/행 수/마지막 글 앞 96자를 기록한다.
  수 MiB 생각 본문을 진단에 다시 저장하지 않는다. 기록은 메모리에 모으고 성공 또는 최초 실패에서 파일로 쓴다.
- 최초 오류·stack과 그때까지의 사건을 DOM 진단보다 먼저 저장한다. 추가 진단은 기존 `inTime` 경계로 제한하고
  진단/파일 쓰기 오류가 최초 실패를 바꾸지 않게 기존 `retentionFailure`를 사용한다. 파일 쓰기 실패를 재시도하지 않는다.
- 실패 시 hash·페이지 시각·hidden/inert·크기·busy·scroll·첫/마지막 행과 `.chat-row:last-child`·뒤쪽 형제·오류를 남긴다.
  원래 본문 대기의 실패와 후속 진단을 구분할 수 있다.
- 파일은 실험실 `diagnostics/chat-lifecycle-<engine>-<width>.jsonl`에 둔다.
  기존 harness가 실패 시 이 폴더를 `AAA_E2E_ARTIFACTS`로 복사하므로 정규 CI artifact에 실린다.

참고한 오픈소스는 [Playwright의 실제 요소 대기](https://playwright.dev/docs/actionability)
([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와
[DOM Testing Library의 DOM 기반 비동기 확인·실패 상태 출력](https://testing-library.com/docs/dom-testing-library/api-async/)
([MIT](https://github.com/testing-library/dom-testing-library/blob/main/LICENSE))다.
기존 locator/DOM 사건 대기를 유지하고 실패 상태를 더 남기는 방식만 참고했으며 소스를 복사하지 않았다.

## 실행 범위와 결과

현재 작업 폴더의 독립 target과 격리 host/agent, 기존 공용 캐시의 headless Chromium/WebKit을 사용했다.
전역 Playwright 1.59.1은 변경하지 않고 작업 폴더의 `target/e2e-tools/node_modules`에 1.64.1 패키지만 두었다.
브라우저 설치·시스템 설정 변경은 없다.

```sh
npm ci --no-audit --no-fund --prefix web
npm --prefix web run build
nice -n 15 cargo build --profile e2e --locked -p aaa --bin aaa
NODE_PATH="$PWD/target/e2e-tools/node_modules" nice -n 15 node web/e2e/web.e2e.mjs --spec 104 --grep 'Chromium 1280' --workers 1
NODE_PATH="$PWD/target/e2e-tools/node_modules" AAA_E2E_THROTTLE=2 nice -n 15 node web/e2e/web.e2e.mjs --spec 104 --grep 'Chromium 1280' --repeat 6 --workers 3
NODE_PATH="$PWD/target/e2e-tools/node_modules" AAA_E2E_THROTTLE=8 AAA_E2E_NO_LOCAL_FONT=1 nice -n 15 node web/e2e/web.e2e.mjs --spec 104 --grep 'Chromium 1280' --workers 1
node --test web/test/retentionFailure.test.mjs web/test/retentionTimers.test.mjs web/test/e2e-layout.test.mjs
```

진단 전 기본 Chromium 1280 단계는 6.549초에 통과했다. 2배 느린 화면·3 worker 대조는 6/6,
파일당 11.3~12.1초였다. 8배 느린 화면·로컬 글꼴 제외 대조는 파일 40.8초에 통과했다.
관측 차이를 주는 조건을 정해 실행했으며 실패 재실행으로 통과시키지 않았다. 이 부하 대조만으로는 원래 CI 실패를 재현하지 못했다.

새 경계와 실패 보존 helper를 포함하는 유닛 12개는 통과했다.
진단을 붙인 E2E104의 Chromium/WebKit 1280/390 네 단계도 통과했다(파일 35.0초).
시나리오를 묶은 기존 단계가 5초를 넘는 사실은 남아 있으며, 이번 진단 보강에서 단언이나 시간을 줄이지 않는다.

## 결정적 RED와 수리 뒤 확인

처음에는 단독 9 MiB의 답을 임시 불일치 본문으로 바꿔 실패 진단 자체를 검증했다.
그때 보존된 DOM에서 답/생각의 역전을 발견했다. 이 본문 변형은 즉시 원복했고 최종 코드에는 없다.

그다음 답을 그대로 유지하고 backstop 응답에 `용량 재검증 반영` 표지만 덧붙였다.
표지가 실제 그려진 사건을 기다린 뒤 원래 `.chat-row:last-child`의 `기록 2399` 대기를 실행했다.
잘못된 `:budget` ID에서는 원래 30초 대기가 결정적으로 실패했다.

- 실패 문맥: Chromium 1280, `scenario=budget-single`, `phase=open-latest-row`, 34번째 열기.
- 실제 DOM: `aria-busy=false`, visible/hidden=false/inert=false, 행 2개, 첫 행 `기록 2399`, 마지막 행 `생각`과 `:budget` ID.
- RED 실행 종료 load는 4.18/17.28/22.47이었다. 실패 순간의 부하로 대체하지 않는다.
  답은 이미 그려졌고 ID순 병합으로 마지막 행이 달라진 상태를 확인했으므로 본문 미도착과 구분한다.
- 최초 stack에 `open`과 단독 용량 단계의 호출 줄이 함께 남았으며, 사건 JSONL이 기존 harness의 실패 artifact 폴더로 복사됐다.
- 재검증 표지 대기는 통과하고 원래 마지막 답 대기가 실패했다. 본문이 없어서 난 실패와 구분했다.

수정은 ID 한 줄이며, 재검증 주입·본문·시한을 그대로 둔 GREEN 및 최종 검증 결과를 아래에 기록한다.


수리 후 동일한 재검증을 유지한 단독 E2E104는 Chromium 1280(6.784초), Chromium 390(7.458초),
WebKit 1280(8.231초), WebKit 390(8.152초)를 모두 통과했다. 파일 전체는 33.541초였다.

```sh
NODE_PATH="$PWD/target/e2e-tools/node_modules" nice -n 15 bash scripts/ci-local.sh --logs <로그 폴더>
```

첫 전체 ci-local은 331초에 web·fmt·build·clippy·test·e2e·public 7단계가 모두 통과했다.
웹 유닛 656개와 E2E 135개 작업이 통과했으며 그 안의 E2E104도 44.924초에 통과했다.
다만 실행 중 회귀 표지 정리와 문서 갱신으로 workspace finish 출처 요약 저장이 거부됐다.
이를 정상 출처 기록으로 세지 않고, 최종 코드 커밋을 고정한 전체 검증을 따로 남긴다.

## 최종 고정 커밋 검증

`2d24b7fa`의 clean 상태를 시작부터 끝까지 유지한 전체 ci-local은 358초에 7단계 모두 통과했다.
workspace 출처는 같은 commit/tree와 `clean=true`로 시작했고, 종료 요약도 `succeeded`로 저장됐다.
첫 실행의 출처 미확인 기록과 구분한다. GitHub rerun·병합·push는 하지 않았다.

| 단계 | 결과 | 초 |
| --- | --- | ---: |
| web(빌드·유닛 656개) | 통과 | 5 |
| fmt | 통과 | 2 |
| build | 통과 | 59 |
| clippy | 통과 | 43 |
| test(Rust·Python 전체) | 통과 | 195 |
| e2e(135개 작업) | 통과 | 289 |
| public | 통과 | 2 |

이 실행의 E2E104는 네 조합 모두 통과했고 파일 작업은 47.593초였다.
이후 변경은 이 검증 기록뿐이며 시험 코드는 고정 커밋과 같다.
시한 연장·기대값 완화·실패 건너뛰기로 통과시키지 않았다.


## 최신 main 착지 통합(t-1298)

착지 창의 기준 원격 main은 `830f920f`다. 그 위에 작업 branch 끝 `915f205a`를 no-ff 통합하며
수리 커밋 `2d24b7fa`도 포함한다. testing.md 충돌은 main의 기존 내용과 t-1298의 링크 한 줄을 모두 보존했다.
마지막 plan §8.544·FRONT_API 1.148·AGENT_LINK 1.11·웹 E2E01~123을 확인했다.
이 수리는 기존 E2E104 fixture와 진단만 고치므로 새 절·API·Agent link·E2E 번호를 배정하지 않는다.

착지 검증은 다른 무거운 검증이 끝난 뒤 LLVM PATH·작업 폴더 Playwright 1.64.1·공용 브라우저 캐시로
salt 없는 nice15 전체 ci-local, 별도 공개 검사, Linux/Windows workspace/all-targets strict cross,
별도 salt 없는 Windows production release, 개인정보 검사 순서로 수행한다.
첫 제품 검증 실패는 원문과 uptime을 남기고 재실행하지 않는다.
통과하면 fast-forward push와 원격 SHA를 대조하고 착지 SHA salt의 Windows 설치기를 격리 전달 폴더에
복사해 SHA·크기·판만 확인한다. 설치·배포·GitHub CI 대기는 하지 않는다. 실행 결과는 t-1298 보고에 남긴다.

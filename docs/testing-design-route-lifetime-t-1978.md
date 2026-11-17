# 기획 branch E2E의 route 수명 경합(t-1978)

E2E 136의 페이지에 API 보류기를 한 번만 등록하고, 단계마다 기다릴 조건만 바꾼다.
그림 업로드와 버리기의 보류 조건이 직접 `fallback`·`continue`하지 않으며, 공통 콜백 끝에서 요청을 한 번 통과시킨다.
제품 코드는 바꾸지 않았다.

## CI 원문과 변경 대조

[실패 실행](https://github.com/cLazyZombie/aaa/actions/runs/44084891588)의
`web-e2e-result-44084891588-1` artifact 안 `e2e.log`를 직접 읽었다.
로컬에 `gh`와 GitHub HTTPS 인증이 없어 `gh run view --log-failed`는 사용할 수 없었고,
공개 Actions API로 실행·job·artifact를 대조한 다음 [같은 artifact](https://nightly.link/cLazyZombie/aaa/actions/runs/44084891588/web-e2e-result-44084891588-1.zip)를 받았다.

```text
route.continue: Route is already handled!
    at hold (web/e2e/specs/136-design-branches.mjs:415:27)
Node.js v24.22.2
```

실패 SHA는 `26d703c9`, job은 `134044624339`의 `web-e2e (macos-15)`이다.
E2E 136은 43.437초 뒤 `ok=false`, `code=1`, `signal=null`로 끝났다.
시작의 load average는 38.45/32.88/19.99, 종료는 26.02/30.25/19.68이다.
처리 안 된 Promise 예외로 프로세스가 끝나 정상적인 단계별 완료 출력·실패 정리가 남지 않았다.

- `fb6ea090`: 대화 목록 닫기와 네 브라우저·화면 조합을 추가했다. 제품의 메뉴 capture 이벤트와
  `ref` 변경에는 HTTP route 처리가 없다. 이를 제품 결함으로 돌릴 근거는 없다.
- `08645b75`: 새로 읽기 전의 배경 갱신까지 보류해 클릭을 막던 교착을 고쳤다.
  pointerdown 이후 보류, 선행 갱신과 보류 요청의 도착 확인은 이번에도 유지했다.
- 앞서 통과한 `5ba5cd77`에도 그림 단계의 `unroute`와 버리기 단계의 재등록·`continue`가 이미 있었다.
  따라서 보류기 교체 경합을 위 두 commit이 처음 만들었다고 단정하지 않는다.

## 재현한 경합과 수리

Playwright 1.64.1은 서버에서 interceptor를 제거할 때 그 handler가 잡고 있는 요청을 자동으로 다음 단계에 넘긴다.
이후 그 요청에 대한 늦은 콜백이 `continue()`하면 이미 처리한 요청이라는 예외가 난다.
애플리케이션 요청의 두 번 전송이나 콜백의 두 번 호출이 없어도 생긴다.

별도 진단에서 실제 loopback HTTP 서버와 실제 Chromium·Mac WebKit을 사용했다.
클라이언트의 `_onRoute` 전달만 Promise로 보류하고, 서버가 요청을 잡은 사건을 기다렸다.
기존 handler를 제거해 실제 HTTP 응답이 끝났음을 확인하고 새 handler를 등록한 다음 사건을 전달했다.
양쪽 모두 콜백 1회로 CI와 같은 예외를 재현했다.
같은 사건 지연에서 handler를 유지하고 보류 조건만 교체하면 양쪽 모두 콜백 1회·응답 성공이다.
private 진단 hook은 재현 전용이며 제품·공용 harness에는 넣지 않았다. 고정 sleep은 없다.

CI에는 요청별 프로토콜 trace가 없어 당시 사건의 정확한 순서까지 복원하지는 못했다.
확인한 것은 원문의 오류, 기존 코드의 교체 구간, 실제 Playwright에서 같은 오류를 내는 결정적 경합이다.
수리는 그 교체 구간을 없앤다. 예외를 삼키거나 이미 처리했는지 보고 건너뛰지 않는다.

페이지의 API route는 처음 한 번 등록한다. `beforeCall`은 요청을 받은 현재 단계의 보류 조건만 실행하고,
공통 콜백이 끝에서 `continue()`한다. 단계 전환 때 interceptor를 제거하지 않는다.
마지막에는 기존대로 gate 해제 → `unrouteAll({ behavior: "wait" })` → context 종료 순서로 정리한다.
통계 pending, 버리기 비활성화, dialog 미생성, 실제 commit 2개 표시와 branch·문서 삭제 단언은 유지한다.
시한·기대값·기존 시나리오를 바꾸거나 skip·재시도를 추가하지 않았다.

## WebKit 반복에서 발견한 통계 준비 경합

최초 일반 반복의 WebKit 두 번째 실행은 `main에 반영` 단계에서 파일 목록이 빈 문자열이라 실패했다.
route 예외는 없었다. 실제 반영 창의 `ChangedFiles`는 받은 branch의 `changed`를 그대로 그리고,
host의 cached 목록 뒤 uncached 저장소 통계가 도착하면 갱신된다. 시험은 dialog 생성만 기다렸기 때문에
그 사이의 빈 목록을 최종 결과로 단언했다. 기존 시험도 같은 순서였다.

반영 창의 변경 행에 추가·삭제 통계가 도착한 사건을 기다린 뒤 같은 경로·숫자 정규식 단언을 실행한다.
고정 시간 대기·기대값 완화가 아니다. 최초 실패 실행은 그대로 보존하고 수정 뒤 반복을 별도로 기록한다.

## 검증 방법

모든 검사는 `aaa verify -- <명령>`으로 실행한다. Playwright 1.64.1 npm 패키지만 작업 target에 두고,
기존 공용 Chromium·WebKit 캐시를 쓴다. 전역 npm·브라우저·시스템 설정은 바꾸지 않는다.
실행 파일은 이 worktree에서 빌드한 e2e CLI의 바이트 대조 사본을 사용한다.

- 원본 `26d703c9`: `ci-local.sh --only web,e2e --spec 136`에서 웹 744건과 전체 18단계 통과.
  간헐 실패이므로 이 통과를 CI 실패의 반증으로 쓰지 않는다.
- route 수리 직후 관련 owner: `verify.py develop --base 26d703c9 --spec 136`에서 웹 타입·번들,
  웹 744건·E2E 18단계·공개 검사 통과.
- 양 브라우저의 버리기 경로까지 실행하기 위해, 진단 adapter가 기본 Browser의 `newContext`를
  별도 실제 WebKit Browser에 위임한다. 공용 harness·제품 파일은 바꾸지 않는다.
  기존 단계 이름에 Chromium이 있어도 adapter 실행에서는 해당 context가 실제 WebKit임을 로그에 명시한다.
- 일반 조건은 Chromium·WebKit 전체 spec을 각각 새 실험실에서 3회씩 직렬 실행한다.
- 부하 조건은 같은 여섯 실행을 동시에 시작하고 Chromium에는 기존 CDP CPU 4배 지연을 적용한다.
  WebKit에는 CDP 지연을 적용하지 않으며 실제 동시 실행 부하로 확인한다.
  각 실행의 종료 코드·경과·load를 남기고 실패를 재실행해 통과로 바꾸지 않는다.

통계 대기까지 수리한 판의 일반 반복은 Chromium 3/3, WebKit 3/3 통과했다.
각 전체 spec은 Chromium 39.95~40.06초, WebKit 42.75~44.15초였다.
그 전의 반복은 Chromium 3/3, WebKit 2/3였으며 위 통계 준비 경합 실패를 포함한다.

부하 조건도 Chromium 3/3, WebKit 3/3 통과했다. 전체 spec은 Chromium 219.07~220.34초,
WebKit 219.43~220.41초였다. 여섯 실행 종료 시 load average는 43.00/23.71/12.28이다.
일반·부하의 최종 12회 모두 기존 18단계를 끝냈다. CPU 지연과 동시 실행을 의도적으로 더한 부하 결과이며,
다른 CPU 수의 CI 환경과 같은 처리량이라고 해석하지 않는다.

통계 대기 수리 뒤 `node --test web/test/e2e-layout.test.mjs web/test/fixed-sleeps.test.mjs`도 14/14 통과했다.
미커밋 후보 검증은 출처 규칙에 따라 workspace 원장에 수집되지 않았으며 로컬 결과를 보존했다.
전체 착지 gate는 착지 창의 최종 통합 SHA에서 수행한다.

## 참고한 공개 구현

- [Playwright Route](https://github.com/microsoft/playwright/blob/v1.64.1/packages/playwright-core/src/server/network.ts),
  [PageDispatcher](https://github.com/microsoft/playwright/blob/v1.64.1/packages/playwright-core/src/server/dispatchers/pageDispatcher.ts),
  [client RouteHandler](https://github.com/microsoft/playwright/blob/v1.64.1/packages/playwright-core/src/client/network.ts)
  (Apache-2.0): interceptor 제거의 자동 통과와 완료 대기의 범위를 확인해, 요청 handler의 수명을 페이지에 맞췄다.
- [Puppeteer HTTPRequest](https://github.com/puppeteer/puppeteer/blob/main/packages/puppeteer-core/src/api/HTTPRequest.ts)
  (Apache-2.0): 비동기 보류 조건과 최종 interception 처리를 나누는 구조를 대조했다.

외부 코드를 복사하거나 새 런타임 의존성을 추가하지 않았다.

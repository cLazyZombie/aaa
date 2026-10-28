# E2E12 부하 뒤 events 수신과 route 정리(t-1232)

## 원본과 원인

- t-1225 착지 2회차의 main `0d8c6418` 원본 실험실을 읽었다. reload 문서의
  events 연결1/2가1006으로 끝나고 연결3까지 열렸다. 첫 마지막 실제 응답은
  reload 시작부터 약22.6초 뒤여서 기존20초 경계를 넘었다. 새 연결은 새 전체 읽기를
  더하며, 이전 읽기의 HTTP도 이어져 여러 목록 읽기가 겹쳤다. host에는3초 무응답 종료가5회 남았다.
- 제품 `events_loop`는 `biased` select에서 알림·ping·presence 기한 뒤에
  socket 수신을 뒀다. host가 늦게 깨어나 만료 tick과 pong이 모두 준비되어도
  tick이 먼저 이겨 연결을 끊는다. 알림이 계속 준비되면 수신도 밀릴 수 있다.
  원본에는 close 직전 수신 큐 계측이 없어 각 close의 pong 도착 시각까지 소급 확정하지 않는다.
- 실제 제품 루프의 첫 state 송신을 gate로 멈추고 pong·앱 ping을 넣는다.
  정지된 Tokio 시계를 기존 무응답 기한보다 앞으로 옮기고 변경 알림도 준비한 뒤 푼다.
  수정 전 `overdue_tick_reads_queued_pong_before_notifications_and_disconnect`가
  앱 pong 없이 종료되어 실패했다. 수신을 기기 취소 바로 뒤로 옮긴 수리 뒤 통과한다.
  wire·ping 간격·무응답 기한·presence lease·목록 페이지 수/크기는 바꾸지 않는다.
- 원본 route 오류도 별개로 재현했다. Playwright1.64.1에서 `unrouteAll(wait)`는
  클라이언트 등록을 먼저 비우고 handler를 기다린다. 먼저 끝난 handler가 interception을
  갱신하면 다른 보류 요청이 서버에서 자동 continue되고 늦은 fulfill이 중복 처리된다.
  두 실제 HTTP 응답 보류와 첫 응답만 해제로 Chromium에서 같은 오류가 났다.

## 수리와 회귀 소유

- events의 준비된 수신을 알림·만료 tick보다 먼저 처리한다. 기기 취소는 최우선이다.
  실제 socket과 같은 Stream/Sink 계약으로 같은 제품 루프를 실행해 별도 시험용 루프를 만들지 않는다.
- `web::events_tests` 세 keeper가 필요한 이유:
  - `overdue_tick_reads_queued_pong_before_notifications_and_disconnect`: 준비된 생존 응답을
    버리는 제품 결함을 부하 재현용 실제 sleep 없이 막는다.
  - `overdue_tick_still_disconnects_a_silent_peer`: 우선순위 수리가 실제 죽은 연결의 종료를 없애지 않는다.
  - `revoked_device_closes_before_queued_input`: 준비된 입력이 기기 취소보다 먼저 처리되지 않는다.
- E2E12의 `ThreadRoutes`는 보류를 모두 푼 뒤 진행 중인 handler가 각 요청을 마칠 때까지
  기다리고 등록을 해제한다. 정리 중 새 요청은 원래 서버로 한 번 통과시킨다.
  정리 중 route 오류도 기록하며 최초 오류가 없으면 실패로 올린다.
- 두 실제 엔진의 `thread-routes.test.mjs`는 보류 응답 두 개·정리 중 새 요청을 쓰며
  마지막 fulfill 전 미해제, 실제 본문, 오류0개, 서버 요청 중복0회를 확인한다.
  필요한 이유는 handler Promise만 흉내 내면 Playwright 서버의 자동 continue를 놓치기 때문이다.
  같은 keeper를 기존 E2E12 본문 수명 단계에도 연결한다. 새 E2E 번호·단계는 없다.
- 기존 reload 문서 판정, 두 완전 목록의 본문 수집, 모든 페이지의 정확한 순서·고유값,
  실제 DOM 전체 목록 단언과20초/120초 실패 상한은 유지한다.
  재시도·시한 확대·기대값 완화·고정 sleep·시험 삭제는 없다.

## 참고

- [Tokio select의 biased 공정성](https://docs.rs/tokio/latest/tokio/macro.select.html#fairness)
  (MIT): 준비가 잦은 분기와 종료·입력의 순서를 호출자가 책임지는 경계.
- [websockets/ws 생존 확인](https://github.com/websockets/ws#how-to-detect-and-close-broken-connections)
  ([MIT](https://github.com/websockets/ws/blob/master/LICENSE)): pong 수신으로 생존 상태를 갱신하는 방식.
- [Playwright unrouteAll](https://playwright.dev/docs/api/class-browsercontext#browser-context-unroute-all)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))와 고정1.64.1의
  RouteHandler/BrowserContext/Route 구현: handler 완료와 등록 해제/서버 자동 continue의 경계.
- 외부 코드·실행 의존성은 추가하지 않는다.

## 확인

- 부분 검증과 정규 착지는 구별한다. 전체 ci-local·cross·main 병합/push는 coordinator의
  착지 창 뒤 지정된 정규 회차에서 수행한다. 새 검증 원문은 이 문서와 thread library에 둔다.
- 웹 유닛649개, 타입/번들, `aaa-host --lib`748통과/기존2ignored,
  host all-targets strict clippy, 의존 경계2개, fmt·diff 공백 검사가 통과했다.
- 새 host keeper3개는 합계0.69초, 실제 route keeper는 Chromium0.210초·WebKit0.624초로 통과했다.
  실패 red/green 원문은 thread library의 `events-red/green.log`와 `routes-red/green.log`에 둔다.
- `NODE_PATH=<자기 Playwright1.64.1> nice -n 15 node web/e2e/web.e2e.mjs <자기 산출물>
  --spec 12-threads-push,17-push-in-use,18-push-stalled --workers 3`가52.6초에3/3 통과했다.
  공용 내부 브라우저 캐시와 기존 Chromium launcher를 사용했다. 시작/끝 load1은12.68/11.25다.
  E12 전체52.6초, CLI 대량 목록4.124초, Chromium 목록5.417초·WebKit 목록5.872초다.
  새 route 수명 keeper를 포함한 기존 본문 수명 단계도 통과했다.
  목록 단계의 이 병렬 측정값은5초를 넘으며, 이를5초 이하 성능 검증으로 표시하지 않는다.
- E17의 실제 무응답 연결 종료와 재연결, E18의 WebSocket pong과 앱 글 lease 구별이 모두 통과했다.
  이 결과로 운영의 모든 부하 지연을 제거했다고 주장하지 않는다. 원본과 같은 준비된 pong/만료 경계는
  제품 루프의 정지 시계 red/green으로 검증하며 정규 착지 부하는 다음 회차에서 다시 확인한다.
- 공개 저장소 검사와 충돌 표지4종(`<<<<<<<`·`=======`·`>>>>>>>`·`|||||||`) 감사도 통과했다.

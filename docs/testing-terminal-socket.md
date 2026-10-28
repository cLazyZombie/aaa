# E2E12 연결 중 취소와 네이티브 재연결(t-1211)

- 현재 `Term.tsx`의 모든 전송은 OPEN 상태를 확인한다. 닫힌 네이티브 소켓에 직접 `send`한 대조는
  Chromium·WebKit에서 예외 없이 버려지지만 Playwright 1.64.1 대체 소켓은 예외를 던진다.
  이 차이만으로 제품의 닫힌 소켓 전송을 원인으로 단정하지 않는다.
- 원래 E12의 events `routeWebSocket`을 건 실제 host 페이지에서 pane 소켓을 만들고 같은 task에서
  `close`하면, 전송 호출 없이 두 엔진 모두 `WebSocket is already in CLOSING or CLOSED state`가 발생했다.
  WebKit stack은 `_apiSendToPage`다. Playwright는 대상 밖 소켓도 대체하며, 취소된 CONNECTING 소켓을
  늦은 passthrough가 서버에 연결하고 그 snapshot을 CLOSING 소켓에 전달한다. 같은 네이티브 순서는
  실제 close 사건으로 끝난다. t-1198의 남은 산출물에는 최초 실패 원문이 없어 당시 stack까지 소급 확정하지 않는다.
- E12는 관측 중인 실제 events 소켓을 브라우저에서 닫고 close 사건을 기다린다. 클라이언트에서 허용하는
  시험 코드 4012를 쓴다(이전 route의 서버 코드 1012). 자동 재연결·새 연결의 첫 HTTP 응답 보류·옛 마지막 응답
  전달 순서와 두 완전 목록의 전체 개수·차례·고유값·커서·본문·실제 DOM 단언은 그대로 검사한다.
- 기존 Chromium/WebKit 목록 단계에 `threadConnectingPaneLifetime`을 더한다. 필요한 이유는
  연결 중 취소한 실제 pane 소켓이 전송 없이도 오류를 내거나 종료되지 않는 대체 소켓의 재도입을 막기 위해서다.
  원래 route를 건 대조에서 이 keeper는 두 엔진 모두 위 수신 예외로 실패했다. close 사건과 pageerror로 끝나며
  새 실패 상한은 5초다. 기존 시한·재시도·기대값 완화·새 E2E 번호·제품 변경·의존성 추가는 없다.
- 참고는 [Playwright 1.64.1 WebSocketMock](https://github.com/microsoft/playwright/blob/v1.64.1/packages/injected/src/webSocketMock.ts)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/v1.64.1/LICENSE)),
  [reconnecting-websocket의 OPEN 검사](https://github.com/pladaria/reconnecting-websocket/blob/master/reconnecting-websocket.ts)
  ([MIT](https://github.com/pladaria/reconnecting-websocket/blob/master/LICENSE)),
  [react-use-websocket의 전송 검사](https://github.com/robtaussig/react-use-websocket/blob/master/src/lib/use-websocket.ts)
  ([MIT](https://github.com/robtaussig/react-use-websocket/blob/master/LICENSE))다.
  상태 검사와 대체/네이티브 수명 차이를 대조했고 외부 코드는 가져오지 않았다.

## 착지 1회차의 문서·연결 준비 경계 수리

- 첫 전체 회차에서 이전 Document의 연결이 세 번 바뀌었다. `armed` 뒤 이전 문서의 마지막 응답이
  `old-final-held`를 먼저 끝냈고, 그 뒤 reload 문서가 시작됐다. 새 events open 전에 disconnect가
  실행돼 「끊을 events 연결이 열려 있지 않다」로 실패했다. 원래 수신 예외와 별개인 시험 준비 결함이다.
- `ThreadReloadGate`가 이전 문서를 제외하고 reload 문서의 마지막 응답과 같은 문서의 새 첫 요청만
  보류한다. reload가 끝난 실제 Document 표지를 다시 대조한다. events disconnect도 그 표지를 확인하며
  해당 문서의 네이티브 open 사건을 기다린 뒤 앱 onopen 처리가 끝나는 microtask에서 close한다.
  close 메서드 반환이 아니라 실제 close 사건으로 끝낸다. 기존 20초 상한과 모든 목록 단언은 유지한다.
- 새 gate keeper는 첫 실패 trace와 같은 이전 응답→새 문서 끝 응답→이전/새 첫 요청 순서를 소유한다.
  기존 trace 유닛의 새 keeper는 CONNECTING→open→close 순서와 앱 onopen 선행·close 사건 완료를 검사한다.
  필요한 이유는 각각 다른 Document의 늦은 응답이 gate를 끝내는 회귀와 연결 준비 전 close를 막기 위해서다.
  두 keeper 모두 수리 전 같은 오류로 실패했고 수리 뒤 5개 trace/gate 유닛 전체가 1초 안에 통과했다.
  실패 재실행·sleep·시한 확대·기대값 완화·E2E 번호 변경은 없다.

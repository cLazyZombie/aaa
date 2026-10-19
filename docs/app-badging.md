# 설치 앱 아이콘 배지

설치한 aaa 앱 아이콘에는 host가 센 **안 본 사용자 보고서 + 아직 처리할 사용자 결정** 수를 표시한다. 0이면 배지를 지운다. 창 focus만으로 지우지 않는다.

## 숫자와 공유

- 보고서는 기존 `--to-user` 보고서의 `sent_at`·`seen_at` 기록을 쓴다. 다른 기기·공유 보고서에서 실제 문서를 열어도 함께 줄어든다. 링크 미리 보기는 열람이 아니다.
- 결정은 처리 전 `needs_user` post(inbox `post request for user`), thread의 「확인 필요」 상태, 아직 ack하지 않은 `[결정 필요]`·`[검수 대기]`·`[확인 필요]` 보고, 열린 확인창 질문이다. 같은 pane의 thread·질문·보고는 한 건이다. post는 별도 요청으로 한 건씩 센다. 단순 열람은 결정 처리가 아니다. post는 `inbox done`, 질문은 답변·종료, thread는 상태 해소·보고 ack·resolve로 줄어든다.
- 웹은 자체 카운터를 두지 않는다. 인증된 `GET /api/v1/app-badge`와 events WS의 `app_badge {count}`가 같은 계산을 쓴다. 최초 연결·재연결과 변경 시 갱신한다. API 응답은 `no-store`다.
- favicon은 기존 의미를 유지한다: 그 창이 focus 밖에서 받은 알림 자리 수이며 focus로 지운다. 설치 앱 배지는 여러 기기에 공유하는 아직 볼 일 수이므로 숫자가 다를 수 있다.

## 지원 범위

다음은 공식 문서 기준이며 실제 OS 배지 모양은 기기에서 확인해야 한다. API가 없거나 호출이 거부되면 조용히 건너뛴다.

| 환경 | API·아이콘 | 숫자·점 | 조건 |
|---|---|---|---|
| macOS Chrome 설치 앱 | 지원, Dock | 숫자 요청 가능 | Chrome 81 이상, 앱 설치 |
| macOS Edge 설치 앱 | 지원, Dock | 숫자 요청 가능 | Edge 81 이상, 앱 설치 |
| macOS Safari 「Dock에 추가」 | 지원, Dock | 숫자 요청 가능 | macOS Sonoma·Safari 17 이상 웹 앱, 알림/배지 허용 설정 확인 |
| Windows Chrome 설치 앱 | 지원, 작업 표시줄 | 숫자 요청 가능 | Chrome 81 이상, 앱 설치, 작업 표시줄 배지 표시 설정 확인 |
| Windows Edge 설치 앱 | 지원, 작업 표시줄 | 숫자 요청 가능 | Edge 81 이상, 앱 설치, 작업 표시줄 배지 표시 설정 확인 |
| iOS·iPadOS 홈 화면 웹 앱 | 지원 | 숫자 요청 가능 | 16.4 이상, 홈 화면 추가·알림 허용; 일반 브라우저 탭에는 API 없음 |
| Android 설치 앱 | Badging API 미지원 | 읽지 않은 OS 알림의 점, 런처에 따라 표시 차이 | 앱이 숫자 제어 불가, 알림을 지우면 OS가 반영 |

지원 API의 `setAppBadge()` 무인자 호출은 점·flag를 요청한다. aaa는 숫자를 전달하고 OS가 점이나 `99+` 등으로 표현할 수 있다. 숫자가 실패했다고 무인자 호출로 재시도하지 않는다.

근거: [Chrome Badging API](https://developer.chrome.com/docs/capabilities/web-apis/badging-api), [Safari 17 웹 앱](https://webkit.org/blog/14445/webkit-features-in-safari-17-0/), [WebKit 홈 화면 Badging](https://webkit.org/blog/14112/badging-for-home-screen-web-apps/).

## 앱이 닫힌 동안

기존 정책이 허용한 coordinator 사용자 post의 Web Push payload에 `app_badge`를 함께 넣는다. worker는 창 없이도 API로 최신 수를 읽고, 오프라인이면 payload 수를 쓴다. 지연 push가 이미 읽은 항목을 되살릴 가능성을 줄인다. 배지 실패가 알림을 막지 않으며 기존 알림도 그대로 표시한다.

읽음·처리·thread 사건만으로 배지 전용 외부 push를 보내지 않는다. 따라서 **닫힌 다른 기기는 다음 허용 push 또는 앱 실행 전까지 옛 배지가 남을 수 있다**. host·브라우저가 완전히 종료되거나 OS가 push를 전달하지 않으면 즉시 갱신을 보장하지 않는다. 읽음만 알리는 조용한 push는 브라우저의 사용자 가시 알림 조건과 aaa의 외부 전달 정책에 맞지 않는다.

## 실기기 확인

1. 새 빌드가 배포된 뒤 HTTPS 주소에서 앱을 설치한다. Safari는 「파일 → Dock에 추가」를 쓴다. 기존 앱은 새 웹 빌드를 다시 연다.
2. 알림을 사용할 기기는 앱의 알림을 켜고 OS의 해당 앱 배지 표시를 허용한다. 자동 시험은 이 설정을 바꾸지 않는다.
3. 기존 볼 일을 처리해 0으로 만든 다음 사용자 보고서 한 건을 보낸다. Dock/작업 표시줄에서 1 또는 OS가 고른 표지를 확인한다.
4. 창에 focus만 주면 배지가 남는지 확인한다. 다른 기기에서 그 보고서를 열면, 열려 있는 앱들의 배지가 사라지는지 확인한다.
5. `needs_user` 요청이나 「확인 필요」 thread를 만들고 수 증가를 본다. 해당 요청을 처리한 뒤 감소를 확인한다.
6. 앱 창을 닫고 coordinator의 허용된 사용자 post를 보낸다. OS 알림과 배지 갱신을 확인한다. 다른 기기에서 읽기만 했을 때 닫힌 앱은 다음 push/실행까지 남을 수 있다.

## 검증 경계

서버 계산은 Rust 유닛, 미지원·거부와 창 없는 worker push는 웹 유닛, 실제 host·DB·WS 및 Chromium/WebKit 두 기기의 열람 동기화·결정 처리·암호화 push 수는 E2E 33이 소유한다. OS Dock/작업 표시줄 모양과 실제 브라우저 push 전달은 위 실기기 절차로 확인한다.

참고 소스의 고정 herdr-web-ui `aa624cb` 및 t3code 웹 소스에는 `setAppBadge`/`clearAppBadge` 구현이 없어 기존 aaa host 기록과 공식 API를 연결했다. 탭 favicon은 기존 t3code 기반 동작을 유지한다.

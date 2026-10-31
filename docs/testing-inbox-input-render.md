# 다른 pane의 inbox 알림과 대화 입력 지연(t-1352)

## 원인

2026-10-31 사용자가 지목한 두 메시지의 입력 시각은 09:44:19와 09:45:24(KST)였다.
그 사이 09:44:28에 main coordinator로 실제 `[aaa inbox]` 입력·Enter가 전달됐고,
프로젝트 흐름에 `nudge_sent`, `accepted`가 기록됐다. 같은 구간에 t-1352로 들어온 자동 알림은 없었다.
따라서 현재 입력창의 알림 주입 충돌이 아니라 다른 pane의 상태 갱신 경로를 재현했다.

1. host는 coordinator가 작업 상태로 바뀌면 전체 pane 상태를 보낸다(`web.rs`의 `send_state`).
2. `App.tsx`의 `watchEvents`가 pane 목록을 갱신하고 보관 중인 `TermView`를 다시 그린다.
3. `TermView`가 받은 파일·링크 열기 함수는 부모의 새 렌더마다 새 참조였다.
4. `ChatPanel`이 이 함수를 `OpenFileContext`·`OpenLinkContext` 값으로 넘겼다.
5. 본문 문자열이 그대로여도 Context를 읽는 `Markdown.tsx::Inline`이 다시 실행됐다.
   숨겨 보관한 coordinator 대화까지 링크·파일 경로를 다시 처리해 입력과 화면 갱신에 쓰는 브라우저 스레드를 점유했다.

`Markdown` 자체의 `memo`만으로는 하위 Context 소비자의 갱신을 막지 못한다.
CPU 프로파일에서도 `Inline`, `URL`, Context 전파와 앱 참조·파일 경로 처리가 확인됐다.

## 수정

`Term.tsx`에서 대화에 넘기는 파일·링크 열기 함수의 참조를 유지한다.
실제 호출은 ref에 둔 최신 부모 함수로 위임한다. 권한이 없어 함수가 없는 경우에는 계속 없는 값으로 넘긴다.
다른 pane 상태 갱신은 본문 Context를 바꾸지 않고, 실제 클릭은 최신 pane·파일 보기 동작을 사용한다.

inbox 전송·확인 주기, 자동 알림 조건, PTY 입력, backend·protocol은 바꾸지 않는다.
작성 중 알림도 정상적으로 도착해야 한다.

## 비교 실험

실제 host·agent·PTY와 앱을 사용하고 별도 측정 번들에 React production profiling을 켰다.
숨긴 coordinator 답 150개와 현재 pane 답 40개를 실제 앞 기록 API로 읽었다.
다른 pane만 바꾼 상태 사건과 숨은 coordinator의 실제 inbox 전달을 각각 실행했다.
계측기와 비교용 함수 참조 유지 변경은 작업 폴더의 `target/inbox-profile/`에만 두었다.

| 측정 | 기존 | 함수 참조 유지 |
| --- | ---: | ---: |
| 다른 pane 상태 갱신 3회 렌더 중앙값 | 26.5ms | 3.1ms |
| 실제 inbox 한 번의 렌더 합계(첫 비교) | 50.8ms | 7.2ms |
| CPU 4배 제한 실험의 상태 갱신 시작→commit | 약 138~145ms | 약 20~22ms |

프로파일링에는 추가 비용이 있다. 위 수치는 사용자 브라우저의 정지 시간을 직접 잰 값이나 모든 기기의 지연 보장이 아니다.
같은 긴 대화와 갱신 경로에서 불필요한 본문 갱신을 제거한 비교다.
실제 알림 도착, 현재 초안·커서·focus 보존과 native 입력도 함께 확인했다.

## 회귀 시험

E2E115는 실제 앱에서 coordinator 대화를 숨겨 보관하고 다른 pane의 글 가운데를 작성한다.
IME 조합이 열린 상태에서도 coordinator의 알림 수신과 작업 상태 반영을 기다리며 초안·커서·편집 가능 상태를 검사한다.
고정된 본문 링크의 URL 생성 횟수로 불필요한 재처리를 검사한다. 짧은 시간 임계값을 올리거나 재시도로 통과시키지 않는다.
같은 본문이 처음 표시될 때 탐침이 실행됐음을 먼저 확인한다.

기존 판에서 Chromium 데스크톱은 알림 중 같은 링크를 84번 다시 처리해 `84 !== 0`으로 실패했다.
수정 판은 재처리가 없어야 한다. 이어 현재 pane의 `context.txt`를 열어 참조를 유지한 함수의 동작도 검사한다.
네 조합은 Chromium·WebKit × 데스크톱·휴대폰이다.

이 시험은 다른 pane의 알림을 허용하면서 입력과 본문 처리 비용을 보호하는 계약에 필요하다.
알림·idle 전환은 실제 출력과 host 상태 사건으로 끝내며 고정 sleep을 쓰지 않는다.

## 참고

- [herdr-web-ui `ChatView.tsx`, aa624cb](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/ChatView.tsx), MIT:
  상태·poll 갱신에서 바뀌지 않은 대화를 건너뛰는 `ChatView`·`Turn` 경계를 확인했다.
  이 수정은 aaa에서 비용을 일으킨 Context 값의 참조만 유지한다. 외부 코드를 복사하거나 의존성을 추가하지 않았다.
- [React `memo`](https://react.dev/reference/react/memo): props가 같아도 구독하는 Context가 바뀌면 소비자가 갱신된다.
- [React `Profiler`](https://react.dev/reference/react/Profiler): production profiling의 추가 비용과 렌더 측정 범위를 따른다.

## 최종 관련 검사

- 웹 타입·번들·웹 유닛 664개: 통과.
- 관련 E2E09(IME), E2E23(대화 전송·첨부·파일 보기), E2E103(대화 보관·복귀), E2E115: 모두 통과.
- 새 E2E115의 네 조합: 각각 1.1~2.5초, 모두 통과. 알림 중 고정 본문 링크 재처리 0회.
- 공개 검사와 `git diff --check`: 통과.
- Git 변경 범위 검사: 웹 전용(웹 소스·spec·Markdown 문서). backend·protocol 변경 없음.

착지 gate는 착지 창의 최종 통합 SHA에서 수행한다. 개발 관련 검사 결과를 전체 착지 검증으로 쓰지 않는다.

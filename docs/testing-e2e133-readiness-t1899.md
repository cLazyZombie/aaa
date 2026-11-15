# 계정 이름 변경 E2E의 초기 사용량 반영 경계(t-1899)

## 최초 실패와 진단

질문 감시 후보를 병합한 전체 gate에서 `133-account-rename`의 Mac WebKit 단계가 실패했다.
이름 변경 버튼 클릭 뒤 `locator.fill`이 30000ms 안에 입력칸을 찾지 못했다.
화면에는 원래 계정과 이름 변경 버튼이 있고 편집 form은 없었다. 이 실패를 재시도로 통과 처리하지 않았다.

먼저 원래 시험에 native pointer/mouse/click과 DOM 변경 기록만 추가한 단독 진단을 실행했다.
이 실행은 통과했으므로 최초 실패의 원인을 단정하지 않았다. 대신 별도 사용량 응답이 초기 목록 뒤에 도착하면서
각 행이 42px 커지는 것을 확인했다. 기존 시험은 계정 목록만 확인하고 이 응답이 반영되기 전에 누를 수 있었다.

다음 진단에서는 실제 host의 첫 사용량 응답을 route에서 보류했다가 이름 변경 버튼의 pointerdown 때 전달했다.
사람의 짧은 press를 흉내 내기 위해 이 진단의 첫 누름에만 100ms 간격을 넣었고, 시험 단언과 fill 시한은 유지했다.
아래 순서로 **같은 입력칸 누락 실패를 재현**했다.

| 사건 | 관측 |
| --- | --- |
| pointerdown 581ms | 이름 변경 BUTTON |
| 사용량 반영 585ms | 앞 행마다 높이 302→344, 313→355px |
| pointerup 687ms | 같은 계정의 P |
| click 688ms | BUTTON 대신 LI, 편집 form 없음 |
| 기존 fill | 30000ms 초과, 최초 실패와 같은 입력칸 누락 |

Playwright가 클릭 직전에 확인한 안정성만으로 뒤늦은 응답에 따른 press 중 위치 이동을 막을 수는 없다.
최초 자연 실패의 pointer 기록은 없으므로 그 한 건의 정확한 사건 순서까지 확인했다고 쓰지는 않는다.
보완하는 결함은 같은 실패를 만드는 초기 응답·native 누름 경합이다.

## 수리와 유지하는 계약

제품 UI나 이름 변경 처리기는 바꾸지 않는다. E2E 133의 처음 누름 전에 `domIs()`로 모든 계정 행의
조회 상태와 자동 배정 초기 조회가 DOM에 반영됐음을 기다린다. 시험의 가짜 Claude 계정은 조회 인증이 없으므로
‘조회 인증 필요’가 각 행에 나타나고 자동 배정 checkbox가 enabled가 되는 것이 준비 사건이다.
이 조건은 임의의 시간이나 일정 프레임 수 대신 실제 응답에 따른 DOM 변화로 끝난다.

처음 이름 저장, 기존 thread·pane 이름 유지, 새 thread의 새 이름 사용, 기본 계정·풀 이전, 실제 name_taken 응답,
오류 위치·가림·입력 유지, 취소 후 오류 제거의 단언은 그대로다. Chromium PC·WebKit PC/휴대폰 조합도 유지한다.
저장 실패 뒤 목록 삽입·사용량 변경으로 오류가 밀리는 기존 회귀는 계속 실행한다.

제어한 응답 진단에서는 새 DOM 대기가 시작될 때 보류 응답을 전달했다. 같은 100ms native 누름의
pointerdown과 pointerup이 모두 BUTTON에 도착했고 기존 시험의 세 브라우저·화면 조합이 모두 통과했다.
이 대기를 빼면 응답은 다시 pointerdown 때 전달되어 위 실패가 발생하는 구성이다.
진단 전용 route·이벤트 기록·100ms 간격은 작업 폴더 `target/`에만 두고 커밋하지 않았다.
제품 검증에는 고정 sleep·click 재시도·시한 연장·기대값 완화·skip을 넣지 않았다.

## 검증

검증은 `aaa verify -- <명령>`으로 실행한다. 관련 owner는 E2E 133이며,
착지에서는 질문 감시 backend 변경과 함께 최종 통합 SHA의 전체 gate·양 OS strict cross를 확인한다.
검증 결과는 작업 보고의 착지 기록에 보존한다.

관련 검사 `ci-local.sh --only web,e2e,public --spec 133`에서 웹 타입·번들·유닛 720개,
E2E 133의 Chromium PC·WebKit PC/휴대폰 조합과 공개 검사가 통과했다.
관련 검사는 67초, E2E 작업은 8.7초였으며 부분 검사를 전체 gate 통과로 기록하지 않는다.

## 대조한 공개 구현

- [Playwright actionability](https://playwright.dev/docs/actionability): visible·stable·enabled·hit target의 사전 확인 범위를 확인했다.
  기존 Playwright의 실제 pointer 입력과 DOM 사건 대기를 유지하며 강제 click으로 바꾸지 않았다.
  [Apache-2.0 라이선스](https://github.com/microsoft/playwright/blob/main/LICENSE).
- [React flushSync](https://react.dev/reference/react-dom/flushSync): DOM 갱신 경계를 대조했다.
  재현에서 click 대상이 이미 LI였으므로 React 강제 동기 렌더나 제품 이벤트 처리기 변경은 하지 않았다.
  [MIT 라이선스](https://github.com/facebook/react/blob/main/LICENSE).

외부 코드나 새 의존성은 추가하지 않았다.

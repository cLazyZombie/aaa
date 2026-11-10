# 질문 초안 전송 실패의 진단(t-1746)

## 확인한 실패와 한계

[원본 CI](https://github.com/cLazyZombie/aaa/actions/runs/42919375425/job/130190050609)의
`123-question-drafts`는 Mac Safari(WebKit), 1280×900에서
「보존한 답을 현재 보고에 한 번 전송」의 선택 단언에 실패했다.
실제 값은 `[0, null]`, 기대 값은 `[1, null]`이다. 첫 질문의 A가 저장됐고 시험은 B를 기대했다.
단계 안의 숫자 배열 단언은 `answer.picks` 하나이므로 이 위치로 좁힐 수 있다.

전체 job 로그와 `web-e2e-result-42919375425-1` artifact를 확보했다.
기존 harness는 예외의 `message`만 출력하며 해당 spec에는 실패 진단 저장이 없었다.
따라서 원본 assertion stack, 요청 본문·횟수, 당시 q-ID·보고 revision, 입력·초안의 사건 기록은 없다.
이를 보존된 로그에서 복구할 수는 없다.

## 사건 순서 대조

| 경계 | 원본 CI가 입증하는 것 | 원본에서 빠진 것 |
| --- | --- | --- |
| 답 저장·보고 재게시·자료 교체 | 앞선 각 단계 통과 | 중간 입력·저장소·q-ID 이력 |
| 새로고침·카드 왕복·서버 일부 답 | 각 보존 단언 통과 | 응답과 렌더의 정확한 순서 |
| 전송 실패·복원·선택지 교체 | 오류 표시와 초안 보존, 새 질문 초기화 단언 통과 | 실제 차단 요청과 새 보고 revision |
| B 누름·직접 답 입력·전송 | 코드가 이 순서로 수행됨 | 실제 누름 도달, 전송 직전 선택·저장소 |
| pane 출력·서버 답 | 둘째 직접 답 출력 대기를 통과, 저장된 선택 `[0, null]` | 요청 대상·본문·횟수, 선택이 바뀐 경계 |
| 성공 회신·초안 해제 | 앞의 단언에서 중단 | 둘 다 원본에서 확인하지 못함 |

로컬의 원본 시험은 통과했다. 관측을 추가한 실행도 통과했다.
통과를 근본 원인 수리나 CI 회복의 증거로 쓰지 않는다.
중복 전송·이전 보고 전송·선택 유실·시험의 입력 동기화 중 무엇인지 아직 확정하지 못했다.

## t-1741과의 접점

t-1741은 `question_guard.rs`의 coordinator 응답 문장 분류만 바꿨다.
`check_question_card`는 Claude/Codex turn reply와 coordinator의 session 신원을 요구한다.
이번 fake thread 카드의 경로는 `QuestionCard` → `App.answerThread` → `Host.thread_answer`이며,
이 함수는 전달받은 선택을 답으로 저장한다. 바뀐 두 문장 분류 조건은 이 경로에서 호출되지 않는다.
커밋 귀속만으로 이 변경을 원인으로 볼 근거는 없다.

## 진단 후보

제품 동작과 기존 단언·시한은 유지한다. E2E 123만 다음과 같이 보강한다.

- 새로고침마다 관측기를 붙여 실제 pointer·click·input의 capture/bubble과 DOM 변경을 기록한다.
  관측 시점의 선택, textarea, 브라우저 초안을 함께 읽는다. 입력·저장소·제품 state를 대신 바꾸지 않는다.
- 보고 수집 hash·질문 묶음, thread WebSocket 사건, 답 요청과 응답을 같은 Node 사건 원장에 남긴다.
- 실패하면 브라우저·크기·단계·예외 stack·actual/expected·원장·현재 host·DOM을
  실험실 `diagnostics/draft-<기기>.json`에 보존한다. 기존 CI artifact 경로로 회수된다.
- 기존 서버 저장 단언 뒤 실제 성공 회신과 초안 삭제를 확인하고, 해당 단계의 요청이
  현재 thread·보고 revision·B 선택·직접 답으로 정확히 한 번 발생했는지 단언한다.
  기존 시험 이름의 「한 번」을 요청 횟수에서도 지키기 위해 필요하다.

관측기는 사건의 실행 시간을 바꿀 수 있다. 후보의 통과는 원인 확정을 대신하지 않는다.
원시 로그·진단 JSON은 작업 산출물에만 두고 공개 문서나 보고 library에는 넣지 않는다.

## 검사

개발 owner는 웹 타입·번들·웹 유닛 전체와 E2E 123의 Chromium/Mac WebKit/iPhone WebKit 전체 조합이다.
실행은 `aaa verify -- nice -n 15 python3 scripts/verify.py develop --base ed8947d3 --spec 123`으로 한다.
웹 유닛 718개와 세 브라우저 조합의 전송 계약이 통과했다.

진단 경로는 Mac WebKit의 정상 전송·회신·초안 삭제 뒤 별도의 고의 assertion 실패를 넣어 검사했다.
이 고의 실패는 원본 결함의 재현이 아니다. CI와 같은 artifact 경로에서 stack·actual/expected,
브라우저·크기·단계, 보고·입력·초안·요청·성공 회신·초안 삭제 기록을 확인했다.
고의 실패 코드를 제거한 상태로 최종 owner를 검사한다. 결과 상세는 작업 보고의 검증 요약에 둔다.

착지·push·배포는 하지 않았다. 착지 승인 뒤 최종 통합 SHA에서 `docs/verification.md`의
웹 전용 전체 gate를 한 번 수행한다. 이번 부분 owner 통과를 착지 gate로 부르지 않는다.

## 다음 진단

진단 후보를 승인된 착지 창에서 통합한 뒤 다음 macOS CI의 실패 산출물을 본다.
실패 직전 B 누름 → 렌더·초안 → 요청의 revision·선택·횟수 → host 저장·회신 순으로 대조한다.
B가 요청 전부터 없으면 입력·폼 교체 경계를, 요청이 B인데 저장이 A면 host의 저장 경계를 조사한다.
보고 revision이나 요청 횟수가 다르면 그 분기를 먼저 추적한다. 재실행 통과로 사건을 닫지 않는다.

## 참고

- [Playwright actionability](https://playwright.dev/docs/actionability),
  [Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE): 누름 전 안정성 검사는
  앱의 답 저장·회신 완료를 보장하지 않으므로 요청과 응답을 따로 관측한다.
- [React 상태 보존](https://react.dev/learn/preserving-and-resetting-state),
  [MIT](https://github.com/facebook/react/blob/main/LICENSE): key 변경으로 폼이 교체되는 경계를
  제품의 의미 있는 질문 identity와 실제 DOM 사건으로 확인한다.

외부 코드·새 제품 의존성은 추가하지 않았다.

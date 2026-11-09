# 보고 갱신의 미답 질문 번호·부분 답 유지

진행·Remember·Next만 고친 보고도 전체 본문 hash가 달라져 매번 새 질문 번호를 만들고 부분 답을 지웠다.
질문 본문·선택지·선택지 뒤 근거를 비교하고, 질문이 가리키는 library 파일은 host 사본 교체 전후의 SHA-256으로 비교한다.
같은 미답 질문만 기존 번호의 현재 보고 hash·위치를 옮긴다. 다른 질문의 변경은 그대로인 질문의 서버 답에 영향을 주지 않는다.

원장 변경은 트랜잭션으로 처리한다. 폐기한 번호는 예약 상태로 남겨 재사용하지 않으며, 예전 본문 hash로 돌아와도 다시 열지 않는다.
옛 판을 읽은 답 요청은 revision·position 조건으로 거부한다. Questions 없는 기본 검수는 보고 전체가 판단 대상이라 매 판 새 질문이다.
전달 완료 답의 중복 방지는 기존 `question_repeat`를 유지한다. 웹 초안도 q 번호를 identity에 포함해 같은 URL의 다른 그림에 옛 입력을 붙이지 않는다.

## 필요한 회귀 시험

- `orch::report_tests::pending_questions_keep_ids_and_partial_comments_across_progress_reorder_and_reversion`:
  실제 보고 수집과 짧은 번호 답 경로로 주변 절 갱신, 재해석, 순서 변경, 선택지 변경, 옛 hash 복귀, 닫기·다시 열기를 검사한다.
  부분 선택과 여러 줄 의견이 올바른 질문을 따르고, 변경·폐기한 질문에는 붙지 않는 계약을 지킨다.
- `orch::report_tests::pending_questions_invalidate_only_changed_material_even_without_report_text_changes`:
  동일 자료 재수집은 답을 유지하고, 본문을 그대로 둔 채 그림 bytes만 바꾸거나 누락시키면 해당 질문만 새로 여는지 본다.
- `web/test/questionDraft.test.mjs`: 같은 질문 번호의 보고 갱신은 초안을 복원하고 새 번호에는 옮기지 않는지 검사한다.
- 웹 E2E 123: Chromium·Mac Safari·iPhone Safari에서 번호 유지, 입력칸·커서 유지, 같은 URL의 그림 변경,
  서버 부분 답의 보고 갱신·목록 유지와 나머지 답의 실제 pane 전달을 본다. 준비·수집·화면·출력 사건으로 끝내며 sleep은 넣지 않는다.
- 기존 E2E 53·69: 늦은 답 응답, 질문 원문·그림·의견, CLI 짧은 번호 답, 독립 coordinator 질문의 경계를 함께 확인한다.

## 검사 범위

개발 owner는 `aaa-host`와 주석 계약을 정리한 `aaa-proto`다. `aaa verify`를 통해 관련 crate 시험·strict Clippy,
웹 타입·번들·웹 유닛 전체와 관련 E2E 53·69·123, 공개 검사를 실행한다. 착지 창의 최종 통합 SHA gate는 별도다.
Windows 전용 코드·프로토콜 필드·DB schema·새 의존성은 바꾸지 않았다.

## 결과와 실패 원인

- 관련 Rust owner의 전체 시험과 strict Clippy가 통과했다. 웹 타입·번들·웹 유닛 717개와 E2E 53·69도 통과했다.
- 첫 E2E 123에서 한 질문 시나리오도 보고 머리에 질문 두 개로 선언한 기존 fixture 오류를 확인했다.
  이 형식 경고 때문에 보수적으로 새 번호가 생겼다. 머리의 선언을 실제 질문 개수로 작성하도록 고쳤다.
- 추가한 부분 답 완료 검사의 처음 실패는 fake 셸이 입력 줄 사이에 prompt를 찍어 여러 줄 연속 출력과 일치하지 않아서였다.
  DB·task 원문과 PTY 전달 완료로 실제 여러 줄 답 전달을 확인했다. 화면은 각 줄의 출력 사건을 기다리고
  DB·task 원문에는 줄바꿈을 포함한 같은 문자열을 단언한다. 시한·기대값을 완화하거나 재시도로 넘기지 않았다.
- 수정 뒤 E2E 123이 Chromium·Mac Safari·iPhone Safari 모두 통과했다. 해당 spec 전체 12.9초이며 각 단계는 1.4초 이내다.
- 최종 공개 검사도 통과했다. 착지 전이므로 전체 착지 gate와 배포는 수행하지 않았다.

## 참고

- [React의 안정된 key](https://react.dev/learn/rendering-lists#keeping-list-items-in-order-with-key),
  [MIT](https://github.com/facebook/react/blob/main/LICENSE): 배열 위치가 아닌 항목 identity로 상태를 이어가는 기준을 참고했다.
- [TanStack Query의 query key](https://tanstack.com/query/latest/docs/framework/react/guides/query-keys),
  [MIT](https://github.com/TanStack/query/blob/main/LICENSE): 자료가 바뀌면 identity도 달라져야 한다는 기준을 참고했다.

외부 코드·의존성은 가져오지 않았다.

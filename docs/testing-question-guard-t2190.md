# 질문 감시의 선택적 의견과 거둔 카드 안내

11월 23일 시험 카드 처리 뒤 체감 의견을 요청하고, 확정한 반경에 조건부 정정 안내를 붙였을 때 누락 경고가 떴다. 같은 날 상황이 바뀌어 닫은 카드를 설명하고 열린 새 카드에 답을 받는 안내에서도 닫힌 카드 경고가 떴다. 시험은 점검 기록에 인용된 구절을 보존하고 주변 문장과 링크를 재구성한다.

- `question_guard::tests::optional_feedback_on_completed_results_keeps_decisions_visible`은 직전 완료·확정 문장에 딸린 의견이나 정정 구절을 검사한다. 완료 근거가 없거나 같은 답에 승인·별개 질문·선택지가 있으면 누락 경고를 확인한다.
- `question_guard::tests::observed_withdrawal_needs_a_confirmed_open_successor`는 같은 주제에서 닫힌 번호를 설명한 직후 더 새로 발급된 열린 번호에 답하도록 지정한 경우를 검사한다. 두 번호의 상태가 확인되지 않거나 열린 번호가 다른 질문에만 쓰이거나 닫힌 번호를 실제 답 대상으로 다시 쓰면 경고를 확인한다.
- `post::tests::question_guard_checks_the_successor_of_a_withdrawn_post`는 독립 사용자 요청을 실제 처리하고 새 요청을 접수한 뒤, Host의 coordinator 완료 사건이 위 조건에 따라 경고를 발행하는지 확인한다.

두 판정 시험은 수정 전 코드에서 각각 누락과 닫힌 카드 경고를 재현했다.

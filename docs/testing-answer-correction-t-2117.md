# 답한 질문 카드 정정 검증(t-2117)

## 동작

「답한 것」 카드에서 「답 고치기」를 누르면 카드가 처음 답할 때와 같은 선택 화면이 된다. 바뀐 질문만
「1A→1B · 2A 그대로」로 요약하고, 확인 창을 거쳐 보낸다(`ask_correct`, FRONT_API 1.191).

- 기록: 처음 답은 지우지 않고 `AskRecord.revisions`에 덧붙인다. `answer`가 지금 답이다. 카드 아래 「답 이력」에 시각·기기와 함께 보인다.
- 기한: thread 카드는 그 thread가 열린 동안만 고친다. 보류·시작 중·끝남·실패·기록 없음은 까닭과 함께 잠근다.
  coordinator 확인 필요 post 카드는 중계한 thread(또는 제목에 처음 나온 이 프로젝트의 thread)가 끝나기 전까지,
  연결된 thread가 없으면 답한 것 기록이 남아 있는 동안 고친다. 판정은 host가 읽을 때마다 정하고 저장하지 않는다(`correction`).
- 알림: thread 카드는 그 thread에 「보고의 질문에 대한 사용자 답 정정(1A→1B)」 글을 보낸다. 바뀐 질문의 이전·고친 줄과
  「앞 답으로 한 일을 맞추거나 되돌리고 보고에 적는다」를 싣는다. 두 카드 모두 coordinator inbox에 `question corrected`를 남긴다.
  host가 모은 답한 뒤 일어난 일(그 thread의 새 보고 판 첫 줄, 그 thread가 보낸 coordinator 요청)을 확인 창과 두 알림에 함께 싣는다.
- 여러 기기: 고치기 시작할 때 본 지금 답 시각(`base_at_ms`)이 기록과 다르면 거절한다. 기록을 먼저 바꾸고 보내며,
  thread에 보내지 못하면 바꾼 자리를 되돌린다. 처음 답이 남고 답 이력도 생기지 않는다.
- 전달 대기(t-2111): host는 thread에 들어간 답에만 답한 것 기록을 만든다. 그래서 전달 대기·보류 중인 답에는 고치기 단추가 없다.
- thread 상태가 바뀌면 host가 「답할 것」을 다시 읽게 한다. 열린 화면도 thread가 끝나는 즉시 잠긴다.
- thread brief의 질문 안내에 「답 정정은 앞 답을 대신한다」를 더했다.

## 회귀 owner

| 범위 | 시험 |
| --- | --- |
| 정정 글 | `ask_correct::tests::correction_lists_only_changed_questions_and_what_happened_since` |
| thread 카드 잠금·기준 시각·보내기 실패 되돌리기 | `ask_correct::tests::thread_cards_follow_the_thread_and_never_half_apply` |
| coordinator 카드 이력·inbox·동시 정정·연결 thread | `ask_correct::tests::coordinator_cards_keep_history_notify_and_follow_their_thread` |
| post 제목의 thread 연결 | `ask_correct::tests::post_titles_link_their_first_thread_id` |
| 웹 차이 요약 | `web/test/asksModel.test.mjs` 「답 고치기 차이」 |
| 휴대폰 실제 흐름(답 → 새 보고 → 고치기 → thread 수신·inbox·이력 → 끝내면 잠김) | `web/e2e/specs/53-thread-questions.mjs` 「답 고치기(t-2117)」 |

되돌리기 줄을 빼면 thread 카드 시험이 「1B 2A」 기록으로 실패하는 것을 확인했다. 고정 sleep·재시도·시한 확대는 없다.

## 참고한 프로젝트

넷 다 제출한 답을 고치는 기능은 없다. 비슷한 생각만 따랐고 코드는 가져오지 않았다.

- [herdr-projects v0.2.35](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/threads.rs)([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)):
  후속 지시를 덧붙이기만 하는 기록과 끝난 thread 거부 → 답 이력 덧붙이기와 끝난 thread 잠금.
- [herdr-web-ui 고정 판 prompt.ts](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/server/prompt.ts)([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)):
  제출 전 질문마다 답을 바꾸는 검토 화면 → 바뀐 질문만 요약해 확인.
- herdr-mobile-relay(AGPL-3.0-or-later): 지난 요청·다른 답을 거부하는 방식 → 기준 시각 비교(생각만 참고).

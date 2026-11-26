# 열린 카드 선택지와 채팅 안내의 누락 오경보

11월 26일 감사 기록의 완료 시각을 coordinator 답변과 대조했다. 아래 시험은 경고를 낸 구절과 카드 배치를 유지하고 링크 주소와 무관한 설명을 줄였다. 수정 전에는 일곱 사례 모두 누락 경고가 재현됐다.

| 당시 안내 | 판정 경계 |
| --- | --- |
| 해석이 다를 때 정정 안내 | 「말씀하신 거라면 알려 주세요」 구절을 선택적 정정으로 본다. |
| 검수 요청 뒤의 선택지 | 답변 첫 줄에서 직접 가리킨 카드와 해설 뒤의 선택지 제목을 연결한다. |
| 수리 범위의 선택지 | 같은 범위의 「thread가 낸 선택지」 제목도 연결한다. |
| 미배포 변경 목록 뒤의 짧은 배포 안내 | 「배포할 변경」 문단에 미배포 상태가 적혀 있을 때만 뒤 문단 첫 구절을 안내로 본다. 같은 문구를 단독으로 쓰면 누락을 경고한다. |
| 배포할 때 채팅 지시를 보내라는 두 안내 | 인용한 채팅 명령이 「배포해」인 경우에 해당 구절만 제외한다. |
| 4번 카드에서 B를 고른 뒤 덧붙일 대사 안내 | 현재 목록 항목이나 명시한 표시 번호의 카드 한 장에만 연결한다. |

`question_guard::tests::november_26_*`는 위 일곱 사례, 같은 줄과 별개 문단의 배포 승인, 잘못된 카드 번호, 답했거나 닫힌 카드, 다른 주제의 선택지를 검사한다. 기존에 실제 카드로 이어진 작업·시안 요청은 `observed_work_and_visual_choices_still_require_cards`에 남아 있다.

`post::tests::question_guard_checks_open_card_guidance_and_separate_approval_at_turn_end`는 실제 Host·DB에서 열린 카드를 접수하고 완료 사건을 전달한다. 해설 뒤 선택지는 경고 없이 지나가고, 카드 접수를 포함한 턴의 별도 승인과 닫힌 카드 재안내는 각각 누락·오래된 카드 경고가 난다. 같은 완료 사건의 중복 검사도 유지한다.

질문 카드와 받은함 사건을 구별하는 기존 경로를 따른다. herdr의 세션 상태 표시, herdr-projects의 받은함 사건·본문 분리, herdr-web-ui의 구조화된 선택지를 대조했다. 각각 Apache-2.0, MIT, MIT이며 외부 코드를 추가하지 않았다. [herdr](https://github.com/herdrdev/herdr/tree/v0.9.5) · [herdr-projects](https://github.com/eliasstravik/shepherdr/blob/v0.2.35/src/inbox.rs) · [herdr-web-ui](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/PromptCard.tsx)

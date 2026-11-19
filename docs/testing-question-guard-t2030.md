# 질문 감시의 선택적 모델 안내와 카드 연결

11월 18일 coordinator 응답과 감시 기록을 대조했다. 회귀 시험에는 필요한 문장 형태만 남기고 주소와 주변 설명은 예시로 바꿨다.

| 시각(KST) | 감시가 본 구절 | 회귀 시험의 기대 결과 |
| --- | --- | --- |
| 16:19 | 터미널에서 프로필을 추가하고 오류가 나면 출력을 알려 달라는 요청. 이후 q-4237이 열렸다. | 카드 누락 경고 |
| 17:40 | 기본 모델로 시작했다는 알림과 선택적 모델 변경 안내 | 경고 없음 |
| 18:00 | 이미 답한 q-4272의 B 선택지에 있는 피드백 요청. 앞줄 안내와 분리되어 15행에서 경고했다. | 경고 없음 |
| 22:13 | q-4295와 웹 검수본 링크가 같은 줄에 있는 선택 요청. 검수본 URL의 마침표 뒤에서 카드 번호를 찾았다. | 경고 없음 |

수정 전 `question_guard::tests::observed_model_offer_and_existing_card_options_do_not_warn`가 세 오탐을 재현했다.
`model_offer_keeps_real_requests_for_input_and_terminal_actions`는 실제 터미널 조작 요청과 모델 결정, 같은 문장에 붙인 별도 승인 요청을 확인한다.
`card_options_and_markdown_keep_separate_requests_and_card_statuses`는 복합 링크의 문장부호, 닫힌 카드와 알 수 없는 번호,
선택지의 독립 승인 요청, 별도 문단의 요청을 대조한다.

카드 번호는 기존 답변·교체 이력에서 확인한다. 글의 문장 경계만 계산할 때 기존 agent에서도 쓰는
Rust의 CommonMark 파서 `pulldown-cmark`의 링크 범위를 사용한다. 비교한 `markdown-it`도 CommonMark를 따르며
JavaScript에서 동작한다. [pulldown-cmark (MIT)](https://github.com/pulldown-cmark/pulldown-cmark#license),
[markdown-it (MIT)](https://github.com/markdown-it/markdown-it/blob/master/LICENSE)

관련 owner는 `aaa-host`다. 기존 Host 완료 사건·DB 시험이 열린 카드, 빠르게 답한 카드, 교체된 카드와 별도 승인 경고를 함께 검사한다.

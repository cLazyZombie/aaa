# 질문 감시의 확정 이름 의견 요청

t-2064의 11월 19일 점검에서 coordinator가 이름을 정했다고 알린 뒤
“다른 이름이 좋으시면 말씀해 주세요”를 덧붙였을 때 질문 카드 누락 경고가 발생했다.
같은 날 새 원격 저장소를 만들지 물었을 때는 카드 누락 경고가 맞았고 q-4341이 열렸다.

`question_guard::tests::optional_feedback_on_a_decided_name_does_not_hide_requests`는
실제 이름 의견 문장을 같은 줄, 줄바꿈, 문단 경계에서 확인한다. 앞에 이름 확정 안내가 없거나
별개 제목을 사이에 두면 경고한다. 실제 이름 결정, 선택지, 같은 문장의 별도 승인도 경고한다.

`post::tests::question_guard_accepts_optional_feedback_and_existing_card_reply_methods`는
coordinator 답변 완료 사건에서 실제 Host와 SQLite inbox를 써 이름 의견만 있는 답에는 경고가 없고
배포 승인이나 미결 이름 결정이 있으면 경고가 생기는지 확인한다.

Vale의 문서 범위별 규칙과 retext-profanities의 구절별 예외를 비교했다. 직전 이름 확정 안내와
의견 구절을 함께 확인하기 위해 기존 Rust 문장 검사에서 범위를 제한했다. 외부 의존성 추가는 없다.
[Vale 범위 규칙](https://vale.sh/features/markup), [Vale 라이선스](https://github.com/vale-cli/vale/blob/v3.12.0/LICENSE),
[retext-profanities 예외 옵션](https://github.com/retextjs/retext-profanities#options),
[retext-profanities 라이선스](https://github.com/retextjs/retext-profanities/blob/main/license)
